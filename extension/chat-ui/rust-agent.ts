import { AbstractAgent } from "@ag-ui/client";
import type { BaseEvent, RunAgentInput } from "@ag-ui/client";
import { EventSchemas, RunAgentInputSchema } from "@ag-ui/core";
import { Observable } from "rxjs";
import {
  connect_ui_bridge,
  UiRunRequest,
  type UiActiveRun,
  type UiBridge,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";

type RustAgentConfig = {
  destination: string;
  mode: string;
  capture: () => void;
  bridgeFactory?: () => Promise<UiBridge>;
  recovery?: UiActiveRun;
  onStopFailure: (error: unknown) => void;
};

type Delivery =
  | { phase: "connecting" }
  | { phase: "submitted"; bridge: UiBridge; stopRequested: boolean }
  | { phase: "settled" };

const emptyRecord = (value: unknown): boolean =>
  typeof value === "object" && value !== null && !Array.isArray(value) && Object.keys(value).length === 0;

export class RustAgent extends AbstractAgent {
  private bridgePromise?: Promise<UiBridge>;
  private config: RustAgentConfig;
  private abortActive?: () => void;
  private abortRequested = false;

  constructor(config: RustAgentConfig) {
    super();
    this.config = { ...config };
  }

  clone(): RustAgent {
    const clone: unknown = super.clone();
    if (!(clone instanceof RustAgent) || clone === this) {
      throw new Error("The agent client did not create an independent clone.");
    }
    clone.config = { ...this.config };
    clone.bridgePromise = undefined;
    clone.abortActive = undefined;
    clone.abortRequested = false;
    clone.isRunning = false;
    return clone;
  }

  override abortRun(): void {
    if (this.abortActive) this.abortActive();
    else if (this.isRunning) this.abortRequested = true;
  }

  protected override connect(input: RunAgentInput): Observable<BaseEvent> {
    const recovery = this.config.recovery;
    if (!recovery) return super.connect(input);

    return new Observable<BaseEvent>((subscriber) => {
      const aborted = this.abortRequested;
      this.abortRequested = false;
      let parsedInput: RunAgentInput;
      try {
        parsedInput = RunAgentInputSchema.parse(input);
        if (parsedInput.threadId !== recovery.thread_id) {
          throw new Error("The active run belongs to another conversation.");
        }
      } catch (error) {
        subscriber.error(error);
        return;
      }

      let terminal = false;
      let stopInFlight = false;
      const requestStop = () => {
        if (terminal || stopInFlight) return;
        stopInFlight = true;
        void (async () => {
            const receipt = await recovery.stop();
            if (receipt !== "requested" && receipt !== "already_terminal") {
              throw new Error("The active run returned an invalid stop receipt.");
            }
          })()
          .catch((error) => {
            stopInFlight = false;
            try {
              this.config.onStopFailure(error);
            } catch (reportingError) {
              if (!subscriber.closed) subscriber.error(reportingError);
            }
          });
      };
      this.abortActive = requestStop;

      let watching: Promise<void>;
      try {
        watching = recovery.watch(parsedInput.runId, (encoded: string) => {
          try {
            const payload: unknown = JSON.parse(encoded);
            const event = EventSchemas.parse(payload);
            if ((event.type === "RUN_STARTED" || event.type === "RUN_FINISHED") &&
                (event.threadId !== recovery.thread_id || event.runId !== parsedInput.runId)) {
              throw new Error("The active run emitted an event for another conversation.");
            }
            terminal = event.type === "RUN_FINISHED" || event.type === "RUN_ERROR";
            subscriber.next(event);
            if (terminal) subscriber.complete();
          } catch (error) {
            if (!subscriber.closed) subscriber.error(error);
          }
        });
      } catch (error) {
        subscriber.error(error);
        return () => {
          if (this.abortActive === requestStop) this.abortActive = undefined;
        };
      }
      if (aborted) {
        requestStop();
      }

      void watching
        .then(() => {
          if (!terminal && !subscriber.closed) {
            subscriber.error(new Error("The active run ended without a terminal event."));
          }
        })
        .catch((error) => {
          if (!subscriber.closed) subscriber.error(error);
        });

      return () => {
        recovery.detach();
        if (this.abortActive === requestStop) this.abortActive = undefined;
      };
    });
  }

  run(input: RunAgentInput): Observable<BaseEvent> {
    return new Observable<BaseEvent>((subscriber) => {
      const aborted = this.abortRequested;
      this.abortRequested = false;
      if (aborted) {
        subscriber.complete();
        return;
      }
      const parsed = RunAgentInputSchema.parse(input);
      const lastMessage = parsed.messages.at(-1);
      if (
        !lastMessage ||
        lastMessage.role !== "user" ||
        typeof lastMessage.content !== "string" ||
        !lastMessage.content.trim() ||
        parsed.tools.length > 0 ||
        parsed.context.length > 0 ||
        !emptyRecord(parsed.state) ||
        !emptyRecord(parsed.forwardedProps) ||
        parsed.messages.some(
          (message) => message.role === "tool" || (message.role === "assistant" && message.toolCalls?.length),
        )
      ) {
        subscriber.error(new Error("This run only supports a plain text user message without tools or state."));
        return;
      }
      if (!this.config.destination.trim() || !["configured", "plan"].includes(this.config.mode)) {
        subscriber.error(new Error("A destination and supported mode are required."));
        return;
      }
      if (this.abortActive) {
        subscriber.error(new Error("This agent already has an active run."));
        return;
      }

      const text = lastMessage.content;
      let delivery: Delivery = { phase: "connecting" };
      const requestStop = () => {
        switch (delivery.phase) {
          case "connecting":
            delivery = { phase: "settled" };
            if (this.abortActive === requestStop) this.abortActive = undefined;
            if (!subscriber.closed) subscriber.complete();
            break;
          case "submitted":
            if (!delivery.stopRequested) {
              delivery.stopRequested = true;
              const active = delivery;
              void active.bridge.stop(parsed.runId).catch(error => {
                if (delivery === active) active.stopRequested = false;
                this.config.onStopFailure(error);
              });
            }
            break;
          case "settled":
            break;
        }
      };
      this.abortActive = requestStop;

      void (async () => {
        try {
          const promise = this.bridgePromise ?? (this.bridgePromise = this.config.bridgeFactory
            ? this.config.bridgeFactory() : connect_ui_bridge(this.config.capture));
          const bridge = await promise.catch(error => {
            if (this.bridgePromise === promise) this.bridgePromise = undefined;
            throw error;
          });
          if (subscriber.closed) return;
          if (parsed.threadId !== bridge.thread_id) {
            throw new Error("The agent thread does not match the saved conversation.");
          }

          const request = new UiRunRequest(
            parsed.threadId,
            parsed.runId,
            lastMessage.id,
            this.config.destination,
            text,
            this.config.mode,
          );
          delivery = { phase: "submitted", bridge, stopRequested: false };
          await bridge.run(request, (eventJson: string) => {
            const payload: unknown = JSON.parse(eventJson);
            const event = EventSchemas.parse(payload);
            const terminal = event.type === "RUN_FINISHED" || event.type === "RUN_ERROR";
            if (terminal) delivery = { phase: "settled" };
            subscriber.next(event);
            if (terminal) subscriber.complete();
          });
          delivery = { phase: "settled" };
          if (!subscriber.closed) {
            subscriber.error(new Error("The native run ended without a terminal event."));
          }
        } catch (error) {
          delivery = { phase: "settled" };
          if (!subscriber.closed) subscriber.error(error);
        } finally {
          delivery = { phase: "settled" };
          if (this.abortActive === requestStop) this.abortActive = undefined;
        }
      })();

      return requestStop;
    });
  }
}
