import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { UiPiDialogKind } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import type {
  UiBridge,
  UiPiNotice,
  UiPiOption,
  UiPiRequest,
  UiPiSnapshot,
  UiPiStatus,
  UiPiWidget,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./pi-ui.css";

type PiUiOwner = ReturnType<UiBridge["pi_ui"]>;
type PiOption = Pick<UiPiOption, "value" | "label">;
type PiRequest = Pick<UiPiRequest, "run_id" | "request_id" | "enabled"> & (
  | { kind: "select"; title: string; message: string; options: PiOption[] }
  | { kind: "confirm"; title: string; message: string }
  | { kind: "input"; title: string; message: string; placeholder: string; prefill: string }
  | { kind: "editor"; title: string; message: string; placeholder: string; prefill: string }
);
type PiWidget = Omit<Pick<UiPiWidget, "key" | "lines" | "above_editor">, "lines"> & {
  lines: ReturnType<UiPiWidget["lines"]>;
};
type PiUiView = Pick<UiPiSnapshot,
  "connected" | "stale" | "busy" | "uncertain" | "notice" | "run_id" | "title"
  | "editor_revision" | "editor_preview" | "editor_destination" | "editor_enabled" | "notices_omitted"
> & {
  requests: PiRequest[];
  statuses: Pick<UiPiStatus, "key" | "text">[];
  widgets: PiWidget[];
  notices: Pick<UiPiNotice, "level" | "message">[];
};

function copyRequest(request: UiPiRequest): PiRequest {
  const common = { run_id: request.run_id, request_id: request.request_id, enabled: request.enabled };
  switch (request.kind) {
    case UiPiDialogKind.Select: {
      const options = request.options();
      try {
        return { ...common, kind: "select", title: request.title, message: request.message,
          options: options.map(option => ({ value: option.value, label: option.label })) };
      } finally {
        options.forEach(option => option.free());
      }
    }
    case UiPiDialogKind.Confirm:
      return { ...common, kind: "confirm", title: request.title, message: request.message };
    case UiPiDialogKind.Input:
      return { ...common, kind: "input", title: request.title, message: request.message, placeholder: request.placeholder, prefill: request.prefill };
    case UiPiDialogKind.Editor:
      return { ...common, kind: "editor", title: request.title, message: request.message, placeholder: request.placeholder, prefill: request.prefill };
    default: {
      const exhaustive: never = request.kind;
      return exhaustive;
    }
  }
}

function copySnapshot(snapshot: UiPiSnapshot): PiUiView {
  const requests = snapshot.requests();
  const statuses = snapshot.statuses();
  const widgets = snapshot.widgets();
  const notices = snapshot.notices();
  try {
    return {
      connected: snapshot.connected,
      stale: snapshot.stale,
      busy: snapshot.busy,
      uncertain: snapshot.uncertain,
      notice: snapshot.notice,
      run_id: snapshot.run_id,
      title: snapshot.title,
      editor_revision: snapshot.editor_revision,
      editor_preview: snapshot.editor_preview,
      editor_destination: snapshot.editor_destination,
      editor_enabled: snapshot.editor_enabled,
      notices_omitted: snapshot.notices_omitted,
      requests: requests.map(copyRequest),
      statuses: statuses.map(status => ({ key: status.key, text: status.text })),
      widgets: widgets.map(widget => ({ key: widget.key, lines: widget.lines(), above_editor: widget.above_editor })),
      notices: notices.map(notice => ({ level: notice.level, message: notice.message })),
    };
  } finally {
    requests.forEach(request => request.free());
    statuses.forEach(status => status.free());
    widgets.forEach(widget => widget.free());
    notices.forEach(notice => notice.free());
    snapshot.free();
  }
}

function key(runId: string, requestId: string) {
  return JSON.stringify([runId, requestId]);
}

function PiDialogCard({ request, disabled, sending, onAnswer, onCancel }: {
  request: PiRequest;
  disabled: boolean;
  sending: boolean;
  onAnswer(request: PiRequest, answer: { kind: "select"; value: string } | { kind: "confirm"; value: boolean } | { kind: "text"; value: string }, event: React.MouseEvent<HTMLButtonElement>): void;
  onCancel(request: PiRequest, event: React.MouseEvent<HTMLButtonElement>): void;
}) {
  const [selected, setSelected] = useState<string>();
  const card = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    const active = document.activeElement;
    if (active instanceof Element && active.closest("[data-pi-card]")) return;
    card.current?.querySelector<HTMLButtonElement | HTMLTextAreaElement>("textarea:not(:disabled), button:not(:disabled)")?.focus();
  }, []);
  const [draft, setDraft] = useState(() => request.kind === "input" || request.kind === "editor" ? request.prefill : "");
  const cardDisabled = disabled || !request.enabled || sending;
  return <article ref={card} data-pi-run={request.run_id} data-pi-request={request.request_id} data-pi-card>
    <h3 data-part="title">{request.title}</h3>
    {request.message && <p data-part="body">{request.message}</p>}
    <form data-part="answer-form" data-pi-answer-kind={request.kind === "select" ? "select" : "text"}
      onSubmit={event => event.preventDefault()}>
      {request.kind === "select" && <div data-part="choices" role="group" aria-label={request.title}>
        {request.options.map((option, index) => <button key={`${index}:${option.value}`} type="button"
          data-action="pi-ui-answer" data-pi-kind="select"
          data-pi-value={option.value} data-part="choice"
          aria-pressed={selected === option.value} aria-label={option.value ? undefined : "Empty option"}
          disabled={cardDisabled} onClick={() => setSelected(option.value)}>
          <span data-part="radio-mark" aria-hidden="true" />
          <span data-part="choice-label">{option.label}</span>
        </button>)}
      </div>}
      {request.kind === "confirm" && <div data-part="choices" role="group" aria-label={request.title}>
        {[true, false].map(value => <button key={String(value)} type="button"
          data-action="pi-ui-answer" data-pi-kind="confirm"
          data-pi-value={String(value)} data-pi-bool={String(value)}
          disabled={cardDisabled} onClick={event => onAnswer(request, { kind: "confirm", value }, event)}>
          {value ? "Yes" : "No"}
        </button>)}
      </div>}
      {(request.kind === "input" || request.kind === "editor") && <textarea data-part="draft"
        aria-label={request.title} placeholder={request.placeholder} value={draft}
        disabled={cardDisabled} onChange={event => setDraft(event.target.value)} />}
      <div data-part="actions">
        <button type="button" data-action="pi-ui-answer" data-pi-kind="cancel"
          data-variant="quiet" disabled={cardDisabled}
          onClick={event => onCancel(request, event)}>Cancel</button>
        {request.kind !== "confirm" && <button type="submit" data-pi-kind={request.kind === "select" ? "select" : "text"}
          data-variant="primary"
          data-requires-selection={request.kind === "select" ? "" : undefined}
          disabled={cardDisabled || (request.kind === "select" && selected === undefined)}
          onClick={event => onAnswer(request, request.kind === "select"
            ? { kind: "select", value: selected ?? "" }
            : { kind: "text", value: draft }, event)}>Send</button>}
      </div>
    </form>
  </article>;
}

export function PiUi({ bridge, destination, draft, onApply, composerSlot }: {
  bridge: UiBridge;
  destination: string;
  draft: string;
  onApply(value: string): void;
  composerSlot: HTMLElement | null;
}) {
  const [view, setView] = useState<PiUiView>();
  const [error, setError] = useState("");
  const [sending, setSending] = useState<string | undefined>(undefined);
  const sendingRef = useRef<string | undefined>(undefined);
  const mounted = useRef(false);
  const ownerRef = useRef<PiUiOwner | undefined>(undefined);
  const container = useRef<HTMLElement>(null);
  const focusAfterRemoval = useRef<Element | undefined>(undefined);

  useLayoutEffect(() => {
    mounted.current = true;
    setView(undefined);
    setError("");
    sendingRef.current = undefined;
    setSending(undefined);
    let owner: PiUiOwner | undefined;
    let subscription: ReturnType<PiUiOwner["subscribe"]> | undefined;
    try {
      owner = bridge.pi_ui();
      ownerRef.current = owner;
      const subscriptionOwner = owner;
      subscription = owner.subscribe(snapshot => {
        const active = document.activeElement;
        const next = copySnapshot(snapshot);
        const focusedCard = active instanceof Element ? active.closest<HTMLElement>("[data-pi-card]") : null;
        const runId = focusedCard?.dataset.piRun;
        const requestId = focusedCard?.dataset.piRequest;
        if (active instanceof Element && runId && requestId &&
          !next.requests.some(request => request.run_id === runId && request.request_id === requestId)) {
          focusAfterRemoval.current = active;
        }
        if (mounted.current && ownerRef.current === subscriptionOwner) setView(next);
      });
    } catch (cause) {
      ownerRef.current = undefined;
      owner?.free();
      if (mounted.current) setError(cause instanceof Error ? cause.message : String(cause));
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
    if (previousFocus.isConnected) {
      if (document.activeElement !== previousFocus && document.activeElement !== document.body) focusAfterRemoval.current = undefined;
      return;
    }
    if (document.activeElement !== document.body) {
      focusAfterRemoval.current = undefined;
      return;
    }
    const next = container.current?.querySelector<HTMLButtonElement | HTMLTextAreaElement>('#pi-ui-cards textarea:not(:disabled), #pi-ui-cards [data-action="pi-ui-answer"]:not(:disabled)');
    if (next) {
      focusAfterRemoval.current = undefined;
      next.focus();
    } else if (!view?.requests.length) {
      focusAfterRemoval.current = undefined;
      document.querySelector<HTMLTextAreaElement>("#bridge-question[data-prompt-field]")?.focus();
    }
  }, [view, sending]);

  function answer(request: PiRequest, value: { kind: "select"; value: string } | { kind: "confirm"; value: boolean } | { kind: "text"; value: string }, event: React.MouseEvent<HTMLButtonElement>) {
    const current = view;
    const owner = ownerRef.current;
    if (!mounted.current || !current || !owner || current.busy || current.stale || current.uncertain || !current.connected || !request.enabled || sendingRef.current) return;
    const requestKey = key(request.run_id, request.request_id);
    if (document.activeElement === event.currentTarget) focusAfterRemoval.current = event.currentTarget;
    sendingRef.current = requestKey;
    setSending(requestKey);
    setError("");
    try {
      const result = value.kind === "select"
        ? owner.select(request.run_id, request.request_id, value.value, event.nativeEvent)
        : value.kind === "confirm"
          ? owner.confirm(request.run_id, request.request_id, value.value, event.nativeEvent)
          : owner.text(request.run_id, request.request_id, value.value, event.nativeEvent);
      void result.catch(cause => {
        if (mounted.current && ownerRef.current === owner) setError(cause instanceof Error ? cause.message : String(cause));
      }).finally(() => {
        if (mounted.current && ownerRef.current === owner) {
          sendingRef.current = undefined;
          setSending(undefined);
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

  function cancel(request: PiRequest, event: React.MouseEvent<HTMLButtonElement>) {
    const current = view;
    const owner = ownerRef.current;
    if (!mounted.current || !current || !owner || current.busy || current.stale || current.uncertain || !current.connected || !request.enabled || sendingRef.current) return;
    const requestKey = key(request.run_id, request.request_id);
    if (document.activeElement === event.currentTarget) focusAfterRemoval.current = event.currentTarget;
    sendingRef.current = requestKey;
    setSending(requestKey);
    setError("");
    try {
      void owner.cancel(request.run_id, request.request_id, event.nativeEvent).catch(cause => {
        if (mounted.current && ownerRef.current === owner) setError(cause instanceof Error ? cause.message : String(cause));
      }).finally(() => {
        if (mounted.current && ownerRef.current === owner) {
          sendingRef.current = undefined;
          setSending(undefined);
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

  function applySuggestion(event: React.MouseEvent<HTMLButtonElement>) {
    const current = view;
    const owner = ownerRef.current;
    if (!mounted.current || !current || !owner || !current.editor_enabled || current.busy || current.stale || current.uncertain || !current.connected ||
      !current.editor_revision || (destination !== "pi" && current.editor_destination !== destination)) return;
    setError("");
    try {
      onApply(owner.editor_text(current.run_id, current.editor_revision, destination, event.nativeEvent));
    } catch (cause) {
      if (mounted.current && ownerRef.current === owner) setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  const unavailable = Boolean(view && (!view.connected || view.stale || view.uncertain));
  const disabled = !view || unavailable || view.busy || Boolean(sending);
  const hasContent = Boolean(error || view?.notice || view?.title || view?.requests.length || view?.statuses.length ||
    view?.widgets.some(widget => widget.above_editor) || view?.notices.length ||
    (view?.notices_omitted && view.notices_omitted !== "0") || (view && (!view.connected || view.stale || view.uncertain)));
  const showReload = unavailable || (Boolean(error) && !ownerRef.current);

  const hasSuggestion = view?.editor_preview !== undefined && (destination === "pi" || view.editor_destination === destination);
  const footerWidgets = view?.widgets.filter(widget => !widget.above_editor) ?? [];
  const footer = <section id="pi-ui-widgets-below" aria-label="Pi editor updates" data-pi-surface="">
    {footerWidgets.map(widget => <pre key={widget.key} data-part="widget" data-widget-key={widget.key}>{widget.lines.join("\n")}</pre>)}
    {view && hasSuggestion && <div id="pi-ui-suggestion" data-part="suggestion" data-pi-run={view.run_id}
      data-pi-revision={view.editor_revision} data-pi-destination={view.editor_destination}>
      <details data-part="editor-preview">
        <summary>Suggested text</summary>
        <pre>{view.editor_preview}</pre>
      </details>
      <button type="button" data-action="pi-ui-replace" data-pi-run={view.run_id}
        data-pi-revision={view.editor_revision} data-pi-destination={view.editor_destination}
        disabled={disabled || !view.editor_enabled || (destination !== "pi" && view.editor_destination !== destination)}
        onClick={applySuggestion}>{draft ? "Replace draft" : "Use suggestion"}</button>
    </div>}
  </section>;

  return <><section id="pi-ui" ref={container} aria-label="Pi requests and updates" hidden={!hasContent} data-pi-run={view?.run_id}>
    <h2 id="pi-ui-title" hidden={!view?.title}>{view?.title}</h2>
    <div id="pi-ui-statuses" aria-label="Pi status messages">
      {view?.statuses.map(status => <p key={status.key} data-status-key={status.key}>{status.text}</p>)}
    </div>
    <div id="pi-ui-widgets-above">
      {view?.widgets.filter(widget => widget.above_editor).map(widget => <pre key={widget.key} data-part="widget" data-widget-key={widget.key}>{widget.lines.join("\n")}</pre>)}
    </div>
    <div id="pi-ui-cards">
      {view?.requests.map(request => <PiDialogCard key={key(request.run_id, request.request_id)}
        request={request} disabled={disabled} sending={sending === key(request.run_id, request.request_id)}
        onAnswer={answer} onCancel={cancel} />)}
    </div>
    <div id="pi-ui-notices" aria-label="Pi notices">
      {view?.notices.map((notice, index) => <p key={`${index}:${notice.level}`} data-part="notice" data-level={notice.level}>{notice.message}</p>)}
      {view?.notices_omitted && view.notices_omitted !== "0" && <p data-part="notice-omission">{view.notices_omitted} earlier Pi notices were omitted.</p>}
      {unavailable ? <p role="alert">{view?.notice || "Reload to check Pi requests."}</p>
        : error ? <p role="alert">{error}</p>
          : view?.notice && <p role="status" data-pi-notice>{view.notice}</p>}
    </div>
    {showReload && <button type="button" data-action="pi-ui-reload" onClick={() => window.location.reload()}>Reload</button>}
  </section>{composerSlot && (hasSuggestion || footerWidgets.length > 0) && createPortal(footer, composerSlot)}</>;
}
