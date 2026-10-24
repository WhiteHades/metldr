import { UiAgentProfile } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import { useEffect, useRef, useState } from "react";
import type {
  UiAgentConfiguration,
  UiAgentSettings,
  UiAgentSnapshot,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";

type ProfileFields = { [K in "kind" | "executable" | "workspace" | "session_dir" | "provider" | "model" | "offline"]: NonNullable<UiAgentProfile[K]> };
type SavedAgent = Pick<UiAgentConfiguration, "agent" | "label" | "revision" | "state"> & ProfileFields;
type SettingsView = Pick<UiAgentSnapshot, "busy" | "can_refresh" | "connected" | "uncertain" | "notice">;
type View = { settings: SettingsView; configurations: SavedAgent[] };
type Draft = Omit<SavedAgent, "agent" | "label" | "state" | "kind"> & { revision: bigint; version: number };
type DraftValues = Omit<Draft, "revision" | "version">;
type RowStatus = { kind: "pending" } | { kind: "error"; message: string };
const stateLabels = new Map([
  ["unmanaged", "Not configured"], ["ready", "Ready"],
  ["removed", "Disabled"], ["unavailable", "Unavailable. Check the saved paths."],
]);

function Chevron() {
  return <svg aria-hidden="true" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6" /></svg>;
}

function readProfile(profile: UiAgentProfile | undefined, agent: string): Pick<SavedAgent, "kind" | "executable" | "workspace" | "session_dir" | "provider" | "model" | "offline"> {
  if (!profile) return { kind: agent, executable: "", workspace: "", session_dir: "", provider: "", model: "", offline: false };
  try {
    return {
      kind: profile.kind,
      executable: profile.executable,
      workspace: profile.workspace,
      session_dir: profile.session_dir ?? "",
      provider: profile.provider ?? "",
      model: profile.model ?? "",
      offline: profile.offline ?? false,
    };
  } finally {
    profile.free();
  }
}

function copySnapshot(snapshot: UiAgentSnapshot): View {
  const configurations = snapshot.configurations();
  try {
    return {
      settings: {
        busy: snapshot.busy,
        can_refresh: snapshot.can_refresh,
        connected: snapshot.connected,
        uncertain: snapshot.uncertain,
        notice: snapshot.notice,
      },
      configurations: configurations.map((configuration) => {
        try {
          return {
            agent: configuration.agent,
            label: configuration.label,
            revision: configuration.revision,
            state: configuration.state,
            ...readProfile(configuration.profile(), configuration.agent),
          };
        } finally {
          configuration.free();
        }
      }),
    };
  } finally {
    snapshot.free();
  }
}

function sameConfigurations(a: SavedAgent[] | undefined, b: SavedAgent[]) {
  if (!a || a.length !== b.length) return false;
  return a.every((row, index) => {
    const next = b[index];
    return row.agent === next.agent && row.label === next.label && row.revision === next.revision
      && row.state === next.state && row.kind === next.kind && row.executable === next.executable
      && row.workspace === next.workspace && row.session_dir === next.session_dir
      && row.provider === next.provider && row.model === next.model && row.offline === next.offline;
  });
}

function savedDraft(row: SavedAgent, version: number): Draft {
  return {
    executable: row.executable,
    workspace: row.workspace,
    session_dir: row.session_dir,
    provider: row.provider,
    model: row.model,
    offline: row.offline,
    revision: row.revision,
    version,
  };
}

function makeProfile(agent: string, draft: Draft): UiAgentProfile {
  switch (agent) {
    case "codex": return UiAgentProfile.codex(draft.executable, draft.workspace);
    case "opencode": return UiAgentProfile.opencode(draft.executable, draft.workspace);
    case "pi": return UiAgentProfile.pi(
      draft.executable,
      draft.workspace,
      draft.session_dir,
      draft.provider,
      draft.model,
      draft.offline,
    );
    default: throw new Error(`Unsupported agent: ${agent}`);
  }
}

export function AgentSettings({
  settings,
  onConfigurationsChange,
}: {
  settings: UiAgentSettings;
  onConfigurationsChange?: () => void;
}) {
  const [view, setView] = useState<View>();
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const draftsRef = useRef(drafts);
  const [statuses, setStatuses] = useState<Record<string, RowStatus>>({});
  const [refreshStatus, setRefreshStatus] = useState<RowStatus>();
  const notifiedConfigurations = useRef<SavedAgent[] | undefined>(undefined);
  const version = useRef(0);

  useEffect(() => {
    const subscription = settings.subscribe((snapshot) => {
      const next = copySnapshot(snapshot);
      if (!next.settings.busy && next.settings.connected && !sameConfigurations(notifiedConfigurations.current, next.configurations)) {
        notifiedConfigurations.current = next.configurations;
        onConfigurationsChange?.();
      }
      setView(next);
    });
    return () => {
      subscription.close();
      subscription.free();
    };
  }, [settings, onConfigurationsChange]);

  function updateDraft<K extends keyof DraftValues>(agent: string, row: SavedAgent, field: K, value: DraftValues[K]) {
    const current = draftsRef.current[agent];
    const nextDraft = {
      ...savedDraft(row, current?.version ?? 0),
      ...current,
      version: ++version.current,
      [field]: value,
    };
    const next = { ...draftsRef.current, [agent]: nextDraft };
    draftsRef.current = next;
    setDrafts(next);
  }

  function settleDraft(agent: string, action: "keep" | "reset", row?: SavedAgent) {
    const next = { ...draftsRef.current };
    if (action === "reset") {
      delete next[agent];
      setStatuses((current) => {
        const statuses = { ...current };
        delete statuses[agent];
        return statuses;
      });
    } else if (row) {
      const draft = next[agent];
      if (draft) next[agent] = { ...draft, revision: row.revision, version: ++version.current };
    }
    draftsRef.current = next;
    setDrafts(next);
  }

  async function mutate(row: SavedAgent, action: "save" | "remove") {
    const draft = draftsRef.current[row.agent];
    const captured = draft ?? savedDraft(row, ++version.current);
    setStatuses((current) => ({ ...current, [row.agent]: { kind: "pending" } }));
    let configuration: UiAgentConfiguration | undefined;
    let profile: UiAgentProfile | undefined;
    try {
      configuration = settings.configuration(row.agent, captured.revision);
      if (action === "save") {
        profile = makeProfile(row.agent, captured);
        await configuration.save(profile);
      } else {
        await configuration.remove();
      }
      const current = draftsRef.current[row.agent];
      if (current?.revision === captured.revision) {
        const next = { ...draftsRef.current };
        if (current.version === captured.version) delete next[row.agent];
        else next[row.agent] = { ...current, revision: captured.revision + 1n };
        draftsRef.current = next;
        setDrafts(next);
      }
      setStatuses((current) => {
        const next = { ...current };
        delete next[row.agent];
        return next;
      });
    } catch (error) {
      setStatuses((current) => ({
        ...current,
        [row.agent]: { kind: "error", message: error instanceof Error ? error.message : String(error) },
      }));
    } finally {
      profile?.free();
      configuration?.free();
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
      setRefreshStatus({ kind: "error", message: "Agent settings could not be refreshed." });
    }
  }

  return (
    <details data-settings-editor data-agent-settings>
      <summary>Installed agents<Chevron /></summary>
      <div data-settings-body>
        <button id="chat-agent-refresh" type="button" disabled={!view?.settings.can_refresh || refreshStatus?.kind === "pending"} onClick={() => void refresh()}>
          Refresh
        </button>
        {refreshStatus?.kind === "pending" && <p role="status">Refreshing agent settings.</p>}
        {refreshStatus?.kind === "error" && <p role="alert">{refreshStatus.message}</p>}
        {view?.settings.notice && <p role="status">{view.settings.notice}</p>}
        {view?.settings.uncertain && <p role="alert">The last change may still be in progress. Refresh agent settings before trying again.</p>}
        {view && !view.settings.connected && <p role="status">Refresh to reconnect, or reload the panel.</p>}
        {view?.configurations.map((row) => {
          const draft = drafts[row.agent];
          const status = statuses[row.agent];
          const busy = Boolean(status?.kind === "pending" || view.settings.busy);
          const conflict = Boolean(draft && draft.revision !== row.revision);
          const textValue = (name: "executable" | "workspace" | "session_dir" | "provider" | "model") => draft?.[name] ?? row[name];
          const field = (name: "executable" | "workspace" | "session_dir" | "provider" | "model", label: string) => (
            <>
              <label htmlFor={`chat-agent-${row.agent}-${name}`}>{label}</label>
              <input id={`chat-agent-${row.agent}-${name}`} value={textValue(name)}
                onChange={(event) => updateDraft(row.agent, row, name, event.currentTarget.value)} />
            </>
          );
          return (
            <details key={row.agent} name="chat-agents" data-settings-row data-agent-kind={row.kind}>
              <summary id={`chat-agent-${row.agent}-heading`}>{row.label}<Chevron /></summary>
              <form onSubmit={(event) => { event.preventDefault(); void mutate(row, "save"); }}>
                <fieldset aria-labelledby={`chat-agent-${row.agent}-heading`}>
                  {field("executable", "Executable")}
                  {field("workspace", "Workspace")}
                  {row.agent === "pi" && <>
                    {field("session_dir", "Session directory")}
                    {field("provider", "Provider")}
                    {field("model", "Model")}
                    <label data-settings-toggle htmlFor={`chat-agent-${row.agent}-offline`}>
                    <input id={`chat-agent-${row.agent}-offline`} type="checkbox" checked={draft?.offline ?? row.offline}
                      onChange={(event) => updateDraft(row.agent, row, "offline", event.currentTarget.checked)} />Offline mode</label>
                  </>}
                  {conflict && !busy && <div data-settings-conflict>
                    <p role="status">Saved settings changed. Review them before keeping your edits.</p>
                    <dl>
                      <dt>Saved executable</dt><dd>{row.executable || "None"}</dd>
                      <dt>Saved workspace</dt><dd>{row.workspace || "None"}</dd>
                      {row.agent === "pi" && <>
                        <dt>Saved session directory</dt><dd>{row.session_dir || "None"}</dd>
                        <dt>Saved provider</dt><dd>{row.provider || "None"}</dd>
                        <dt>Saved model</dt><dd>{row.model || "None"}</dd>
                        <dt>Saved offline mode</dt><dd>{row.offline ? "On" : "Off"}</dd>
                      </>}
                    </dl>
                    <button id={`chat-agent-${row.agent}-keep`} type="button" onClick={() => settleDraft(row.agent, "keep", row)}>Keep my edits</button>
                    <button id={`chat-agent-${row.agent}-reset`} type="button" onClick={() => settleDraft(row.agent, "reset")}>Use saved values</button>
                  </div>}
                  <div data-settings-actions>
                    <button id={`chat-agent-${row.agent}-save`} type="submit" disabled={busy || conflict}>Save</button>
                    <button id={`chat-agent-${row.agent}-remove`} type="button" disabled={busy || conflict} onClick={() => void mutate(row, "remove")}>Remove</button>
                    {(!conflict || busy) && <button id={`chat-agent-${row.agent}-reset`} type="button" disabled={busy} onClick={() => settleDraft(row.agent, "reset")}>Use saved values</button>}
                  </div>
                  <p role={status?.kind === "error" ? "alert" : "status"}>
                    {status?.kind === "error" ? status.message : status?.kind === "pending" ? "Updating agent settings." : stateLabels.get(row.state) ?? "Unavailable"}
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
