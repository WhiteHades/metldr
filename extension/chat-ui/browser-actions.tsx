import { useId, useLayoutEffect, useRef, useState } from "react";
import type {
  UiBridge,
  UiBrowserAction,
  UiBrowserActionDetail,
  UiBrowserActionSnapshot,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./browser-actions.css";

type BrowserActionDetail = Pick<UiBrowserActionDetail, "part" | "label" | "value">;
type BrowserAction = Pick<
  UiBrowserAction,
  | "id"
  | "revision"
  | "run_id"
  | "url"
  | "title"
  | "status"
  | "phase"
  | "receipt"
  | "approve_enabled"
  | "decline_enabled"
> & { details: BrowserActionDetail[] };
type BrowserActionView = Pick<
  UiBrowserActionSnapshot,
  "connected" | "stale" | "busy" | "uncertain" | "notice"
> & { actions: BrowserAction[] };

function copySnapshot(snapshot: UiBrowserActionSnapshot): BrowserActionView {
  const actions = snapshot.actions();
  try {
    return {
      connected: snapshot.connected,
      stale: snapshot.stale,
      busy: snapshot.busy,
      uncertain: snapshot.uncertain,
      notice: snapshot.notice,
      actions: actions.map((action) => {
        const details = action.details();
        try {
          return {
            id: action.id,
            revision: action.revision,
            run_id: action.run_id,
            url: action.url,
            title: action.title,
            status: action.status,
            phase: action.phase,
            receipt: action.receipt,
            approve_enabled: action.approve_enabled,
            decline_enabled: action.decline_enabled,
            details: details.map((detail) => ({ part: detail.part, label: detail.label, value: detail.value })),
          };
        } finally {
          details.forEach((detail) => detail.free());
        }
      }),
    };
  } finally {
    actions.forEach((action) => action.free());
    snapshot.free();
  }
}

function actionKey(id: string, revision: string) {
  return JSON.stringify([id, revision]);
}

export function BrowserActions({ bridge, onPendingChange }: {
  bridge: UiBridge;
  onPendingChange?: (pending: boolean) => void;
}) {
  const descriptionId = useId();
  const [view, setView] = useState<BrowserActionView>();
  const [error, setError] = useState("");
  const [setupFailed, setSetupFailed] = useState(false);
  const [sending, setSending] = useState<string>();
  const sendingRef = useRef<string | undefined>(undefined);
  const viewRef = useRef<BrowserActionView | undefined>(undefined);
  const mounted = useRef(false);
  const callback = useRef(onPendingChange);
  const ownerRef = useRef<ReturnType<UiBridge["browser_actions"]> | undefined>(undefined);
  const container = useRef<HTMLElement>(null);
  const focusAfterRemoval = useRef<Element | null>(null);

  callback.current = onPendingChange;

  useLayoutEffect(() => {
    mounted.current = true;
    setView(undefined);
    viewRef.current = undefined;
    setError("");
    setSetupFailed(false);
    sendingRef.current = undefined;
    setSending(undefined);
    callback.current?.(false);

    let owner: ReturnType<UiBridge["browser_actions"]> | undefined;
    let subscription: ReturnType<ReturnType<UiBridge["browser_actions"]>["subscribe"]> | undefined;
    try {
      owner = bridge.browser_actions();
      ownerRef.current = owner;
      const subscriptionOwner = owner;
      subscription = owner.subscribe((snapshot) => {
        const active = document.activeElement;
        const next = copySnapshot(snapshot);
        const focusedCard = active instanceof Element ? active.closest<HTMLElement>("[data-browser-action]") : null;
        const id = focusedCard?.dataset.browserAction;
        const revision = focusedCard?.dataset.revision;
        const nextAction = next.actions.find((action) => action.id === id && action.revision === revision);
        if (active instanceof Element && id && revision && (!nextAction || !nextAction.approve_enabled && !nextAction.decline_enabled)) {
          focusAfterRemoval.current = active;
        }
        const inFlight = sendingRef.current;
        if (inFlight && !next.uncertain && !next.actions.some((action) => actionKey(action.id, action.revision) === inFlight)) {
          sendingRef.current = undefined;
          setSending(undefined);
        }
        if (mounted.current && ownerRef.current === subscriptionOwner) {
          viewRef.current = next;
          callback.current?.(next.actions.some((action) => action.phase === "pending"));
          setView(next);
        }
      });
    } catch (cause) {
      const failedOwner = owner;
      owner = undefined;
      ownerRef.current = undefined;
      failedOwner?.free();
      if (mounted.current) {
        setError(cause instanceof Error ? cause.message : String(cause));
        setSetupFailed(true);
      }
    }

    return () => {
      mounted.current = false;
      try {
        subscription?.close();
      } finally {
        try {
          subscription?.free();
        } finally {
          ownerRef.current = undefined;
          owner?.free();
        }
      }
    };
  }, [bridge]);

  useLayoutEffect(() => {
    const previousFocus = focusAfterRemoval.current;
    if (!previousFocus) return;
    const blurredBecauseDisabled = previousFocus instanceof HTMLButtonElement && previousFocus.disabled;
    if (previousFocus.isConnected && document.activeElement !== document.body && document.activeElement !== previousFocus) {
      focusAfterRemoval.current = null;
      return;
    }
    if (previousFocus.isConnected && document.activeElement === previousFocus && !blurredBecauseDisabled) return;
    focusAfterRemoval.current = null;
    if (document.activeElement !== document.body && document.activeElement !== previousFocus && previousFocus.isConnected) return;
    const next = container.current?.querySelector<HTMLButtonElement>('[data-action="browser-approve"]:not(:disabled), [data-action="browser-decline"]:not(:disabled)');
    if (next) next.focus();
    else document.querySelector<HTMLTextAreaElement>("#bridge-question[data-prompt-field]")?.focus();
  }, [view, sending]);

  function decide(action: BrowserAction, approve: boolean, event: React.MouseEvent<HTMLButtonElement>) {
    if (!mounted.current || !view || view.busy || view.stale || view.uncertain || !view.connected) return;
    const owner = ownerRef.current;
    if (!owner || sendingRef.current) return;
    const key = actionKey(action.id, action.revision);
    if (document.activeElement === event.currentTarget) focusAfterRemoval.current = event.currentTarget;
    sendingRef.current = key;
    setSending(key);
    setError("");
    try {
      void owner.decide(action.id, action.revision, approve, event.nativeEvent).catch((cause: unknown) => {
        if (mounted.current && ownerRef.current === owner) {
          setError(cause instanceof Error ? cause.message : String(cause));
          if (!viewRef.current?.uncertain) {
            sendingRef.current = undefined;
            setSending(undefined);
          }
        }
      });
    } catch (cause) {
      if (mounted.current && ownerRef.current === owner) {
        setError(cause instanceof Error ? cause.message : String(cause));
        sendingRef.current = undefined;
        setSending(undefined);
      }
    }
  }

  const unavailable = Boolean(view && (!view.connected || view.stale || view.uncertain));
  const showReload = unavailable || setupFailed;
  const currentActions = view?.actions.filter((action) => !action.receipt) ?? [];
  const receipts = view?.actions.filter((action) => action.receipt) ?? [];
  const hasContent = Boolean(error || view?.notice || unavailable || currentActions.length || receipts.length);
  if (!hasContent) return null;

  return (
    <section id="browser-actions" ref={container} aria-label="Browser actions">
      {unavailable
        ? <p role="alert">{view?.notice || "Browser actions need a fresh panel state. Reload to check them again."}</p>
        : error
          ? <p role="alert">{error}</p>
        : view?.notice && <p role="status">{view.notice}</p>}
      {showReload && <button type="button" data-action="browser-action-reload" onClick={() => window.location.reload()}>Reload</button>}
      {currentActions.map((action) => {
        const key = actionKey(action.id, action.revision);
        const disabled = !view || unavailable || view.busy || Boolean(sending && sending !== key);
        const accessDescription = action.decline_enabled && !action.approve_enabled
          ? `${descriptionId}-${action.id}-${action.revision}` : undefined;
        const hasSourceOrDestination = action.details.some((detail) =>
          ["source", "destination"].includes(detail.part.toLowerCase()) ||
          ["source", "destination"].includes(detail.label.toLowerCase()),
        );
        return (
          <article key={key} data-browser-action={action.id} data-revision={action.revision} data-phase={action.phase}>
            <h3>{action.title}</h3>
            <p data-part="status">{action.status}</p>
            {!hasSourceOrDestination && <p data-part="url">{action.url}</p>}
            {action.details.map((detail, index) => (
              <p key={`${detail.part}:${index}`} data-row={detail.part}><strong>{detail.label}: </strong><span data-part={detail.part}>{detail.value}</span></p>
            ))}
            {accessDescription && <p id={accessDescription} data-part="source-access">Approval needs access to the original page. You can still decline this action.</p>}
            {action.phase === "pending" && <div data-part="controls">
              <button type="button" data-action="browser-decline" data-browser-action={action.id} data-revision={action.revision} aria-label={`Decline browser action at ${action.url}`} disabled={disabled || !action.decline_enabled || sending === key} onClick={(event) => decide(action, false, event)}>Decline</button>
              <button type="button" data-action="browser-approve" data-browser-action={action.id} data-revision={action.revision} aria-label={`Approve browser action at ${action.url}`} aria-describedby={accessDescription} disabled={disabled || !action.approve_enabled || sending === key} onClick={(event) => decide(action, true, event)}>Approve</button>
            </div>}
          </article>
        );
      })}
      {receipts.length > 0 && (
        <details id="browser-receipts" data-browser-receipts>
          <summary>Browser action results ({receipts.length})</summary>
          <div id="browser-receipt-rows">
          {receipts.map((action) => {
            const hasSourceOrDestination = action.details.some((detail) =>
              ["source", "destination"].includes(detail.part.toLowerCase()) ||
              ["source", "destination"].includes(detail.label.toLowerCase()),
            );
            return (
            <article key={actionKey(action.id, action.revision)} data-browser-action={action.id} data-revision={action.revision} data-phase={action.phase}>
              <h3>{action.title}</h3>
              <p data-part="status">{action.status}</p>
              {!hasSourceOrDestination && <p data-part="url">{action.url}</p>}
              {action.details.map((detail, index) => (
                <p key={`${detail.part}:${index}`} data-row={detail.part}><strong>{detail.label}: </strong><span data-part={detail.part}>{detail.value}</span></p>
              ))}
            </article>
            );
          })}
          </div>
        </details>
      )}
    </section>
  );
}
