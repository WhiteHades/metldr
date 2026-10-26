import { useLayoutEffect, useRef, useState } from "react";
import type {
  UiBridge,
  UiNativePermission,
  UiNativePermissionOption,
  UiNativePermissionSnapshot,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./native-permissions.css";

type PermissionOption = Pick<UiNativePermissionOption, "id" | "label">;
type PermissionRequest = Pick<
  UiNativePermission,
  "run_id" | "request_id" | "agent" | "tool" | "title" | "input" | "enabled"
> & { options: PermissionOption[] };
type PermissionView = Pick<
  UiNativePermissionSnapshot,
  "connected" | "stale" | "busy" | "uncertain" | "notice"
> & { requests: PermissionRequest[] };

function copySnapshot(snapshot: UiNativePermissionSnapshot): PermissionView {
  const requests = snapshot.requests();
  try {
    return {
      connected: snapshot.connected,
      stale: snapshot.stale,
      busy: snapshot.busy,
      uncertain: snapshot.uncertain,
      notice: snapshot.notice,
      requests: requests.map((request) => {
        const options = request.options();
        try {
          return {
            run_id: request.run_id,
            request_id: request.request_id,
            agent: request.agent,
            tool: request.tool,
            title: request.title,
            input: request.input,
            enabled: request.enabled,
            options: options.map((option) => ({ id: option.id, label: option.label })),
          };
        } finally {
          options.forEach((option) => option.free());
        }
      }),
    };
  } finally {
    requests.forEach((request) => request.free());
    snapshot.free();
  }
}

function requestKey(runId: string, requestId: string) {
  return JSON.stringify([runId, requestId]);
}

export function NativePermissions({
  bridge,
  onPendingChange,
}: {
  bridge: UiBridge;
  onPendingChange?: (pending: boolean) => void;
}) {
  const [view, setView] = useState<PermissionView>();
  const [error, setError] = useState("");
  const [sending, setSending] = useState<string>();
  const sendingRef = useRef<string | undefined>(undefined);
  const mounted = useRef(false);
  const ownerRef = useRef<ReturnType<UiBridge["native_permissions"]> | undefined>(undefined);
  const callback = useRef(onPendingChange);
  const container = useRef<HTMLElement>(null);
  const focusAfterRemoval = useRef<Element | null>(null);

  callback.current = onPendingChange;

  useLayoutEffect(() => {
    mounted.current = true;
    setView(undefined);
    setError("");
    sendingRef.current = undefined;
    setSending(undefined);
    callback.current?.(false);

    let owner: ReturnType<UiBridge["native_permissions"]> | undefined;
    let subscription: ReturnType<ReturnType<UiBridge["native_permissions"]>["subscribe"]> | undefined;
    try {
      owner = bridge.native_permissions();
      ownerRef.current = owner;
      const subscriptionOwner = owner;
      subscription = owner.subscribe((snapshot) => {
        const active = document.activeElement;
        const next = copySnapshot(snapshot);
        const focusedCard = active instanceof Element
          ? active.closest<HTMLElement>("[data-native-permission-card]")
          : null;
        const runId = focusedCard?.dataset.nativeRun;
        const requestId = focusedCard?.dataset.nativeRequest;
        if (
          active instanceof Element &&
          runId &&
          requestId &&
          !next.requests.some((request) => request.run_id === runId && request.request_id === requestId)
        ) focusAfterRemoval.current = active;
        if (mounted.current && ownerRef.current === subscriptionOwner) {
          callback.current?.(next.requests.length > 0);
          setView(next);
        }
      });
    } catch (cause) {
      const failedOwner = owner;
      owner = undefined;
      ownerRef.current = undefined;
      failedOwner?.free();
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
      if (document.activeElement !== previousFocus && document.activeElement !== document.body) focusAfterRemoval.current = null;
      return;
    }
    focusAfterRemoval.current = null;
    if (document.activeElement !== document.body) return;
    const next = container.current?.querySelector<HTMLButtonElement>('[data-action="native-permission"]:not(:disabled)');
    if (next) next.focus();
    else document.querySelector<HTMLTextAreaElement>("#bridge-question[data-prompt-field]")?.focus();
  }, [view]);

  function reply(request: PermissionRequest, option: string | undefined, event: React.MouseEvent<HTMLButtonElement>) {
    if (!mounted.current || !view || view.busy || view.stale || view.uncertain || !view.connected || !request.enabled) return;
    const owner = ownerRef.current;
    if (!owner) return;
    const key = requestKey(request.run_id, request.request_id);
    if (sendingRef.current) return;
    if (document.activeElement === event.currentTarget) focusAfterRemoval.current = event.currentTarget;
    sendingRef.current = key;
    setSending(key);
    setError("");
    try {
      void owner.reply(
        request.run_id,
        request.request_id,
        option,
        event.nativeEvent,
      ).catch((cause: unknown) => {
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

  const unavailable = !view?.connected || view.stale || view.uncertain;
  const disabled = !view || unavailable || view.busy || Boolean(sending);
  const showReload = Boolean(view && unavailable) || Boolean(error);
  const recoveryMessage = view?.uncertain
    ? "The permission result is uncertain. Reload the panel before choosing again."
    : view?.stale
      ? "The conversation changed. Reload the panel to check its requests."
      : view && !view.connected
        ? "Connection lost. Reload the panel to reconnect."
        : undefined;
  const hasContent = Boolean(error || view?.notice || view?.uncertain || view?.connected === false || view?.stale || view?.requests.length);

  if (!hasContent) return null;

  return (
    <section id="native-permissions" ref={container} aria-label="Native permission requests">
      {recoveryMessage ? <p role="alert">{recoveryMessage}</p> : <>
        {view?.notice && <p role="status" data-native-notice>{view.notice}</p>}
        {error && <p role="alert">{error}</p>}
      </>}
      {showReload && <button type="button" data-action="native-permission-reload" onClick={() => window.location.reload()}>Reload</button>}
      {view?.requests.map((request) => {
        const key = requestKey(request.run_id, request.request_id);
        const requestDisabled = disabled || !request.enabled || sending === key;
        return (
          <article
            key={key}
            data-native-run={request.run_id}
            data-native-request={request.request_id}
            data-native-permission-card
          >
            <h3 data-part="title">{request.title}</h3>
            <p data-part="agent">{request.agent}</p>
            <p data-part="tool">{request.tool}</p>
            <pre data-part="input" tabIndex={0} role="region" aria-label="Permission request input">{request.input}</pre>
            <div data-native-permission-options>
              {request.options.map((option) => (
                <button
                  key={option.id}
                  type="button"
                  data-action="native-permission"
                  data-native-run={request.run_id}
                  data-native-request={request.request_id}
                  data-native-option={option.id}
                  disabled={requestDisabled}
                  onClick={(event) => reply(request, option.id, event)}
                >{option.label}</button>
              ))}
              <button
                type="button"
                data-action="native-permission"
                data-native-run={request.run_id}
                data-native-request={request.request_id}
                disabled={requestDisabled}
                onClick={(event) => reply(request, undefined, event)}
              >Cancel</button>
            </div>
          </article>
        );
      })}
    </section>
  );
}
