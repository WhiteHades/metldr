import { useLayoutEffect, useRef, useState } from "react";
import { z } from "zod";
import type { UiBridge } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./conversation-history.css";

export const ConversationPage = z.object({
  conversations: z.array(z.object({
    id: z.string(),
    title: z.object({ text: z.string(), truncated: z.boolean() }),
  })),
  olderCursor: z.string().nullable(),
});

type State =
  | { kind: "closed" }
  | { kind: "loading" }
  | { kind: "ready"; page: z.infer<typeof ConversationPage> }
  | { kind: "error"; message: string };

export function ConversationHistory({ bridge, enqueue, disabled, onSelect }: {
  bridge: UiBridge;
  enqueue<T>(operation: () => Promise<T>): Promise<T>;
  disabled: boolean;
  onSelect(id: string): void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const pending = useRef(false);
  const generation = useRef(0);
  const [state, setState] = useState<State>({ kind: "closed" });
  useLayoutEffect(() => () => { generation.current += 1; }, [bridge]);

  function load(cursor?: string) {
    if (pending.current) return;
    pending.current = true;
    const expected = generation.current;
    setState({ kind: "loading" });
    void enqueue(async () => {
      const data: unknown = JSON.parse(await bridge.conversations(cursor));
      return ConversationPage.parse(data);
    }).then(page => {
      if (generation.current === expected) setState({ kind: "ready", page });
    }).catch(error => {
      if (generation.current === expected) setState({ kind: "error", message: String(error) });
    }).finally(() => { pending.current = false; });
  }

  return <>
    <button id="bridge-conversations" type="button" disabled={disabled || state.kind === "loading"} onClick={() => {
      dialog.current?.showModal();
      load();
    }}>Saved chats</button>
    <dialog className="chat-settings chat-history" data-conversation-history="" aria-labelledby="chat-history-title"
      aria-busy={state.kind === "loading"} ref={dialog}>
      <header><h2 id="chat-history-title">Saved chats</h2>
        <button type="button" onClick={() => dialog.current?.close()}>Close</button>
      </header>
      <div className="chat-history__body">
        {state.kind === "loading" && <p role="status">Loading chats</p>}
        {state.kind === "error" && <p role="alert">{state.message}</p>}
        {state.kind === "ready" && <nav aria-label="Saved chats">
          {state.page.conversations.length === 0 && <p>No saved chats</p>}
          {state.page.conversations.map(row => <button type="button" key={row.id} data-conversation-id={row.id}
            disabled={disabled} onClick={() => { dialog.current?.close(); onSelect(row.id); }}>
            {row.title.text || "New chat"}
          </button>)}
        </nav>}
      </div>
      <footer>
        <button id="bridge-conversations-recent" type="button" disabled={state.kind === "loading"} onClick={() => load()}>Recent chats</button>
        <button id="bridge-conversations-older" type="button" disabled={state.kind !== "ready" || !state.page.olderCursor}
          onClick={() => { if (state.kind === "ready" && state.page.olderCursor) load(state.page.olderCursor); }}>Older chats</button>
      </footer>
    </dialog>
  </>;
}
