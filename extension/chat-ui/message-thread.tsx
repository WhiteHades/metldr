import { type ReactNode, useRef } from "react";
import type { Message } from "@ag-ui/core";
import "./message-thread.css";

type MessageThreadProps = {
  messages: readonly Message[];
  running: boolean;
  liveMessageIds: ReadonlySet<Message["id"]>;
  renderActivity: (
    message: Extract<Message, { role: "activity" }>,
  ) => ReactNode;
};

export function MessageThread(
  { messages, running, liveMessageIds, renderActivity }: MessageThreadProps,
) {
  const hydratedIds = useRef<Set<Message["id"]> | undefined>(undefined);
  const hydrated = hydratedIds.current ??= new Set(
    messages.map((message) => message.id),
  );
  const lastMessage = messages.at(-1);
  const waiting = running && lastMessage?.role !== "activity" &&
    (lastMessage?.role !== "assistant" || !lastMessage.content?.length);

  return (
    <section className="message-thread" aria-label="Conversation messages">
      {messages.map((message) => {
        const arrival = liveMessageIds.has(message.id) &&
            !hydrated.has(message.id)
          ? "true"
          : undefined;
        const text =
          typeof message.content === "string"
            ? message.content
            : undefined;
        const textContent = text ?? (
          <span className="message-thread__fallback">
            No plain text content is available.
          </span>
        );

        if (message.role === "user") {
          return (
            <div
              key={message.id}
              className="message-thread__user-row"
              data-message-id={message.id}
              data-message-role={message.role}
              data-arrival={arrival}
            >
              <div className="message-thread__user-bubble">{textContent}</div>
            </div>
          );
        }

        if (message.role === "assistant") {
          return (
            <article
              key={message.id}
              className="message-thread__assistant"
              data-message-id={message.id}
              data-message-role={message.role}
              data-arrival={arrival}
            >
              <p>{textContent}</p>
            </article>
          );
        }

        if (message.role === "activity") {
          return (
            <div
              key={message.id}
              className="message-thread__activity"
              data-message-id={message.id}
              data-message-role={message.role}
              data-arrival={arrival}
            >
              {renderActivity(message)}
            </div>
          );
        }

        return (
          <article
            key={message.id}
            className="message-thread__fallback-message"
            data-message-id={message.id}
            data-message-role={message.role}
            data-arrival={arrival}
          >
            <p className="message-thread__fallback">
              This message type is not displayed.
            </p>
          </article>
        );
      })}
      {waiting && (
        <div
          className="message-thread__loading"
          role="status"
          aria-label="Working"
        >
          <span className="message-thread__dots" aria-hidden="true">
            {Array.from({ length: 9 }, (_, index) => <span key={index} />)}
          </span>
          <span>Working</span>
        </div>
      )}
    </section>
  );
}
