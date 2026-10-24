import { useEffect, useRef, useState } from "react";
import type {
  UiProviderConnection,
  UiProviderSettings,
  UiProviderSnapshot,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";

type SavedConnection = Pick<
  UiProviderConnection,
  "protocol" | "label" | "revision" | "state" | "endpoint" | "model"
>;
type SettingsView = Pick<
  UiProviderSnapshot,
  | "busy"
  | "can_refresh"
  | "connected"
  | "uncertain"
  | "notice"
  | "cleanup_pending"
>;
type Draft = { endpoint: string; model: string; revision: bigint; version: number };
type RowStatus = { kind: "pending" } | { kind: "error"; message: string };
type View = { settings: SettingsView; connections: SavedConnection[] };
const stateLabels = new Map([
  ["unmanaged", "Not configured"], ["ready", "Ready"], ["removed", "Disabled"],
  ["loading", "Loading"], ["missing", "The saved key is missing."],
  ["locked", "The credential store is locked."], ["corrupt", "The saved key could not be read."],
  ["unavailable", "Unavailable"],
]);

function Chevron() {
  return <svg aria-hidden="true" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6" /></svg>;
}

function copySnapshot(snapshot: UiProviderSnapshot): View {
  const connections = snapshot.connections();
  try {
    return {
      settings: {
        busy: snapshot.busy,
        can_refresh: snapshot.can_refresh,
        connected: snapshot.connected,
        uncertain: snapshot.uncertain,
        notice: snapshot.notice,
        cleanup_pending: snapshot.cleanup_pending,
      },
      connections: connections.map((connection) => {
        try {
          return {
            protocol: connection.protocol,
            label: connection.label,
            revision: connection.revision,
            state: connection.state,
            endpoint: connection.endpoint,
            model: connection.model,
          };
        } finally {
          connection.free();
        }
      }),
    };
  } finally {
    snapshot.free();
  }
}

function sameConnections(a: SavedConnection[] | undefined, b: SavedConnection[]) {
  if (!a || a.length !== b.length) return false;
  return a.every((row, index) => {
    const next = b[index];
    return row.protocol === next.protocol
      && row.label === next.label
      && row.revision === next.revision
      && row.state === next.state
      && row.endpoint === next.endpoint
      && row.model === next.model;
  });
}

export function ProviderSettings({
  settings,
  onConnectionsChange,
}: {
  settings: UiProviderSettings;
  onConnectionsChange?: () => void;
}) {
  const [view, setView] = useState<View>();
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const draftsRef = useRef(drafts);
  const [statuses, setStatuses] = useState<Record<string, RowStatus>>({});
  const [refreshStatus, setRefreshStatus] = useState<RowStatus>();
  const keyInputs = useRef<Record<string, HTMLInputElement | null>>({});
  const notifiedConnections = useRef<SavedConnection[] | undefined>(undefined);
  const version = useRef(0);

  useEffect(() => {
    const subscription = settings.subscribe((snapshot) => {
      if (snapshot.reset_keys) {
        Object.values(keyInputs.current).forEach(input => { if (input) input.value = ""; });
      }
      const next = copySnapshot(snapshot);
      if (!next.settings.busy && next.settings.connected && !sameConnections(notifiedConnections.current, next.connections)) {
        notifiedConnections.current = next.connections;
        onConnectionsChange?.();
      }
      setView(next);
    });
    return () => {
      subscription.close();
      subscription.free();
    };
  }, [settings, onConnectionsChange]);

  function updateDraft(protocol: string, row: SavedConnection, field: "endpoint" | "model", value: string) {
    const current = draftsRef.current[protocol];
    const next = {
      ...draftsRef.current,
      [protocol]: {
        endpoint: current?.endpoint ?? row.endpoint ?? "",
        model: current?.model ?? row.model ?? "",
        revision: current?.revision ?? row.revision,
        version: ++version.current,
        [field]: value,
      },
    };
    draftsRef.current = next;
    setDrafts(next);
  }

  function resetDraft(protocol: string) {
    const input = keyInputs.current[protocol];
    if (input) input.value = "";
    const next = { ...draftsRef.current };
    delete next[protocol];
    ++version.current;
    draftsRef.current = next;
    setDrafts(next);
    setStatuses((current) => {
      const nextStatuses = { ...current };
      delete nextStatuses[protocol];
      return nextStatuses;
    });
  }

  function keepDraft(protocol: string, row: SavedConnection) {
    const draft = draftsRef.current[protocol];
    if (!draft) return;
    const next = {
      ...draftsRef.current,
      [protocol]: { ...draft, revision: row.revision, version: ++version.current },
    };
    draftsRef.current = next;
    setDrafts(next);
  }

  async function mutate(protocol: string, row: SavedConnection, action: "save" | "remove") {
    const draft = draftsRef.current[protocol];
    const captured = draft ?? {
      endpoint: row.endpoint ?? "",
      model: row.model ?? "",
      revision: row.revision,
      version: ++version.current,
    };
    const input = keyInputs.current[protocol];
    const key = action === "save" ? input?.value || undefined : undefined;
    if (input) input.value = "";
    setStatuses((current) => ({ ...current, [protocol]: { kind: "pending" } }));

    let connection: UiProviderConnection | undefined;
    try {
      connection = settings.connection(protocol, captured.revision);
      if (action === "save") {
        await connection.save(captured.endpoint, captured.model, key);
      } else {
        await connection.remove();
      }
      const current = draftsRef.current[protocol];
      if (current?.revision === captured.revision) {
        const next = { ...draftsRef.current };
        if (current.version === captured.version) {
          delete next[protocol];
        } else {
          // A confirmed native mutation advances once; unrelated changes stay conflicts.
          next[protocol] = { ...current, revision: captured.revision + 1n };
        }
        draftsRef.current = next;
        setDrafts(next);
      }
      setStatuses((current) => {
        const nextStatuses = { ...current };
        delete nextStatuses[protocol];
        return nextStatuses;
      });
    } catch (error) {
      setStatuses((current) => ({
        ...current,
        [protocol]: { kind: "error", message: error instanceof Error ? error.message : String(error) },
      }));
    } finally {
      connection?.free();
    }
  }

  async function refresh() {
    setRefreshStatus({ kind: "pending" });
    try {
      await settings.refresh();
      setRefreshStatus(undefined);
      setStatuses((current) => Object.fromEntries(
        Object.entries(current).filter(([, status]) => status.kind === "pending"),
      ));
    } catch {
      setRefreshStatus({ kind: "error", message: "Provider settings could not be refreshed." });
    }
  }

  return (
    <details data-settings-editor data-provider-settings>
      <summary>AI providers<Chevron /></summary>
      <div data-settings-body data-provider-body>
      <button
        id="chat-provider-refresh"
        type="button"
        disabled={!view?.settings.can_refresh || refreshStatus?.kind === "pending"}
        onClick={() => void refresh()}
      >
        Refresh
      </button>
      {refreshStatus?.kind === "pending" && <p role="status">Refreshing provider settings.</p>}
      {refreshStatus?.kind === "error" && <p role="alert">{refreshStatus.message}</p>}
      {view?.settings.notice && <p role="status">{view.settings.notice}</p>}
      {view?.settings.uncertain && <p role="alert">The last change may still be in progress. Refresh provider settings before trying again.</p>}
      {view?.settings.cleanup_pending && <p role="status">A saved provider entry is waiting to be removed.</p>}
      {view && !view.settings.connected && <p role="status">Refresh to reconnect, or reload the panel.</p>}
      {view?.connections.map((row) => {
        const draft = drafts[row.protocol];
        const status = statuses[row.protocol];
        const busy = Boolean(status?.kind === "pending" || view.settings.busy);
        const conflict = Boolean(draft && draft.revision !== row.revision);
        return (
          <details key={row.protocol} name="chat-providers" data-settings-row data-provider-protocol={row.protocol}>
            <summary id={`chat-provider-${row.protocol}-heading`}>{row.label}<Chevron /></summary>
            <form onSubmit={(event) => { event.preventDefault(); void mutate(row.protocol, row, "save"); }}>
            <fieldset aria-labelledby={`chat-provider-${row.protocol}-heading`}>
              <label htmlFor={`chat-provider-${row.protocol}-endpoint`}>Endpoint</label>
              <input
                id={`chat-provider-${row.protocol}-endpoint`}
                value={draft?.endpoint ?? row.endpoint ?? ""}
                onChange={(event) => updateDraft(row.protocol, row, "endpoint", event.currentTarget.value)}
              />
              <label htmlFor={`chat-provider-${row.protocol}-model`}>Model</label>
              <input
                id={`chat-provider-${row.protocol}-model`}
                value={draft?.model ?? row.model ?? ""}
                onChange={(event) => updateDraft(row.protocol, row, "model", event.currentTarget.value)}
              />
              <label htmlFor={`chat-provider-${row.protocol}-key`}>API key</label>
              <input
                id={`chat-provider-${row.protocol}-key`}
                type="password"
                autoComplete="new-password"
                onChange={() => {
                  const existing = draftsRef.current[row.protocol];
                  updateDraft(row.protocol, row, "endpoint", existing?.endpoint ?? row.endpoint ?? "");
                }}
                ref={(input) => { keyInputs.current[row.protocol] = input; }}
                disabled={busy}
              />
              {conflict && !busy && <div data-settings-conflict data-provider-conflict>
                <p role="status">Saved settings changed. Review them before keeping your edits.</p>
                <dl>
                  <dt>Saved endpoint</dt><dd>{row.endpoint || "None"}</dd>
                  <dt>Saved model</dt><dd>{row.model || "None"}</dd>
                </dl>
                <button id={`chat-provider-${row.protocol}-keep`} type="button"
                  onClick={() => keepDraft(row.protocol, row)}>Keep my edits</button>
              </div>}
              <div data-settings-actions data-provider-actions>
              <button
                id={`chat-provider-${row.protocol}-save`}
                type="submit"
                disabled={busy || conflict}
              >Save</button>
              <button
                id={`chat-provider-${row.protocol}-remove`}
                type="button"
                disabled={busy || conflict}
                onClick={() => void mutate(row.protocol, row, "remove")}
              >Remove</button>
              <button
                id={`chat-provider-${row.protocol}-reset`}
                type="button"
                disabled={busy}
                onClick={() => resetDraft(row.protocol)}
              >Use saved values</button>
              </div>
              <p role={status?.kind === "error" ? "alert" : "status"}>
                {status?.kind === "error"
                  ? status.message
                  : status?.kind === "pending"
                    ? "Updating provider settings."
                    : stateLabels.get(row.state) ?? "Unavailable"}
              </p>
            </fieldset>
            </form>
          </details>
        );
      })}
      </div>
    </details>
  );
}
