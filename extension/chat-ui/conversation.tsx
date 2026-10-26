import { useCallback, useEffect, useMemo, useRef, useState, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { CopilotKitProvider, CopilotChatConfigurationProvider, useCopilotKit, useAgent, useRenderActivityMessage } from "@copilotkit/react-core/v2";
import { MessageSchema } from "@ag-ui/core";
import { z } from "zod";
import init, { connect_ui_bridge, UiPreferences, type UiBridge, type UiProviderSettings, type UiAgentSettings } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import { RustAgent } from "./rust-agent";
import { SavedTable, TableReference } from "./saved-table";
import PromptBar from "./prompt-bar";
import type { PromptChoice } from "./prompt-bar";
import { ProviderSettings } from "./provider-settings";
import { AgentSettings } from "./agent-settings";
import "./data-table.css";
import "./settings-editor.css";
import { InteractionSounds } from "./interaction-sounds";
import { ChatLayout } from "./chat-layout";
import { ConversationHistory, ConversationPage } from "./conversation-history";
import { MessageThread } from "./message-thread";
import { NativePermissions } from "./native-permissions";
import { BrowserActions } from "./browser-actions";
import { PiUi } from "./pi-ui";
import { CodexInput } from "./codex-input";
import { NativeToolChips } from "./native-tool-chips";
import { RunDetails } from "./run-details";
import { useConnection } from "./connection";
import { useReviewDraft } from "./review-draft";
import { SavedSource } from "./saved-source";
import { PreferenceSettings, usePreferences } from "./preferences";
import "./run-details.css";
import "./chat-theme.css";

type Capture = () => { url: string; title: string; text: string };
type NavigationControls = Pick<React.ComponentProps<typeof ChatLayout>, "tabs" | "activeId" | "onSelect" | "onClose" | "onNew" | "navigationDisabled"> & { error: string; retry(): void; openSettings(): void };

const ArchivePage = z.object({
  messages: z.array(MessageSchema),
  olderCursor: z.string().nullable(),
});
const Snapshot = ArchivePage.extend({
  threadId: z.string(),
  running: z.boolean(),
  sourceAvailable: z.boolean(),
});

type Enqueue = <T>(operation: () => Promise<T>) => Promise<T>;

async function executionChoices(bridge: UiBridge): Promise<PromptChoice[]> {
  const choices = await bridge.execution_choices();
  try {
    return choices.map(choice => ({ value: choice.value, label: choice.label, enabled: choice.enabled, supports_plan: choice.supports_plan }));
  } finally {
    choices.forEach(choice => choice.free());
  }
}

function executionDisplay({ choices, value, previous }: { choices: PromptChoice[]; value: string; previous?: PromptChoice }) {
  const known = choices.find(choice => choice.value === value);
  const selection = known ?? { value, label: previous?.label ?? "Saved destination unavailable", enabled: false, supports_plan: false };
  return { choices, selection };
}

function Conversation({ initial, failure, bridge, recovery, enqueue, destination, initialChoices, controlRevision, navigation, onHistoryChange }: { initial: z.infer<typeof Snapshot>; failure: string; bridge: UiBridge; recovery?: ReturnType<UiBridge["active_run"]>; enqueue: Enqueue; destination: string; initialChoices: PromptChoice[]; controlRevision: number; navigation: NavigationControls; onHistoryChange(): Promise<void> }) {
  const connection = useConnection(bridge);
  const { agent } = useAgent({ threadId: initial.threadId });
  const { copilotkit } = useCopilotKit();
  const { renderActivityMessage } = useRenderActivityMessage();
  const loaded = useRef<typeof agent | null>(null);
  const mounted = useRef(false);
  const [ready, setReady] = useState(false);
  const { text, setText, error: reviewError, review } = useReviewDraft(initial.threadId);
  const [piComposerSlot, setPiComposerSlot] = useState<HTMLDivElement | null>(null);
  const [codexMode, setCodexMode] = useState<"configured" | "plan">("configured");
  const [error, setError] = useState("");
  const [archive, setArchive] = useState<z.infer<typeof ArchivePage>>(initial);
  const [reading, setReading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [approvalPending, setApprovalPending] = useState(false);
  const [browserApprovalPending, setBrowserApprovalPending] = useState(false);
  const [nativeRunning, setNativeRunning] = useState(initial.running);
  const [execution, setExecution] = useState(() => executionDisplay({ choices: initialChoices, value: destination }));
  const { selection } = execution;
  const choices = execution.choices.some(choice => choice.value === selection.value)
    ? execution.choices : [...execution.choices, selection];
  const archiveReads = useRef(0);
  const draftRevision = useRef(0);
  const pending = useRef<{ messageId: string; revision: number } | null>(null);
  const recoveryStopPending = useRef(false);
  const choicesRevision = useRef(0);
  const choicesWanted = useRef(controlRevision);
  choicesWanted.current = controlRevision;
  const choicesPending = useRef(false);
  const [choicesTick, setChoicesTick] = useState(0);
  useEffect(() => {
    if (!ready || capturing || reading || submitting || nativeRunning || agent.isRunning || choicesPending.current || controlRevision === choicesRevision.current) return;
    choicesPending.current = true;
    void enqueue(() => executionChoices(bridge)).then(next => {
      if (!mounted.current) return;
      choicesRevision.current = controlRevision;
      setExecution(old => executionDisplay({ choices: next, value: old.selection.value, previous: old.selection }));
    }).catch(error => {
      if (mounted.current) { choicesRevision.current = controlRevision; setError(String(error)); }
    }).finally(() => {
      choicesPending.current = false;
      if (mounted.current && choicesRevision.current !== choicesWanted.current) setChoicesTick(value => value + 1);
    });
  }, [ready, capturing, reading, submitting, nativeRunning, agent.isRunning, controlRevision, choicesTick, enqueue, bridge]);
  useLayoutEffect(() => {
    mounted.current = true;
    const subscription = agent.subscribe({
      onRunStartedEvent: ({ event, input }) => {
        const submission = pending.current;
        if (mounted.current && submission && event.threadId === initial.threadId &&
            input.messages.at(-1)?.id === submission.messageId &&
            draftRevision.current === submission.revision) setText("");
      },
      onRunFailed: ({ error }) => { if (mounted.current) setError(error.message); },
      onRunFinishedEvent: () => { if (mounted.current) setNativeRunning(false); },
      onRunErrorEvent: ({ event }) => {
        if (mounted.current) { setNativeRunning(false); setError(event.message); }
      },
    });
    return () => {
      mounted.current = false;
      subscription.unsubscribe();
      void agent.detachActiveRun();
      recovery?.detach();
    };
  }, [agent, recovery, initial.threadId]);
  useEffect(() => {
    if (loaded.current === agent) return;
    loaded.current = agent;
    agent.setMessages([]);
    setReady(true);
    if (recovery) {
      void agent.connectAgent().then(() => {
        if (!mounted.current) return;
        void readArchive(latest).catch(error => {
          if (mounted.current) setError(current => current || String(error));
        });
      }).catch(error => {
        if (mounted.current) setError(current => current || String(error));
      });
    }
  }, [agent, initial]);
  function readArchive(operation: () => Promise<void>) {
    if (!mounted.current) return Promise.resolve();
    archiveReads.current += 1;
    setReading(true);
    return enqueue(operation).finally(() => {
      archiveReads.current -= 1;
      if (mounted.current && archiveReads.current === 0) setReading(false);
    });
  }
  async function latest() {
    const data: unknown = JSON.parse(await bridge.snapshot());
    const next = Snapshot.parse(data);
    if (!mounted.current) return;
    if (next.threadId !== initial.threadId || next.running) {
      throw new Error("The saved conversation is busy or changed.");
    }
    const currentChoices = await executionChoices(bridge);
    if (!mounted.current) return;
    setExecution(old => executionDisplay({ choices: currentChoices, value: old.selection.value, previous: old.selection }));
    setArchive(next);
    setNativeRunning(next.running);
    agent.setMessages([]);
    void onHistoryChange();
  }
  const liveIds = new Set(agent.messages.map(message => message.id));
  const visible = [...archive.messages.filter(message => !liveIds.has(message.id)), ...agent.messages];
  function captureCurrentPage() {
    if (navigation.navigationDisabled || !ready || capturing || reading || submitting || nativeRunning || agent.isRunning) return;
    setError("");
    setCapturing(true);
    void (async () => {
      await bridge.capture();
      await readArchive(latest);
    })().catch(error => { if (mounted.current) setError(String(error)); })
      .finally(() => { if (mounted.current) setCapturing(false); });
  }
  function submit() {
    if (navigation.navigationDisabled || !text.trim() || !ready || capturing || reading || pending.current || submitting || nativeRunning || agent.isRunning) return;
    if (!(agent instanceof RustAgent)) throw new Error("The conversation requires its Rust agent.");
    const messageId = crypto.randomUUID();
    agent.prepareRun({ destination: selection.value, mode: selection.supports_plan ? codexMode : "configured" });
    pending.current = { messageId, revision: draftRevision.current };
    setError("");
    setSubmitting(true);
    agent.addMessage({ id: messageId, role: "user", content: text });
    void copilotkit.runAgent({ agent }).then(() => readArchive(latest))
      .catch(error => { if (mounted.current) setError(current => current || String(error)); })
      .finally(() => {
        if (pending.current?.messageId === messageId) pending.current = null;
        if (mounted.current) setSubmitting(false);
      });
  }
  function applyPiSuggestion(value: string) {
    draftRevision.current += 1;
    setText(value);
    document.querySelector<HTMLTextAreaElement>("#bridge-question[data-prompt-field]")?.focus();
  }
  function selectExecution(choice: PromptChoice) {
    if (controlsDisabled) return;
    try {
      bridge.select_execution_choice(choice.value);
      setExecution(old => executionDisplay({ choices: old.choices, value: choice.value, previous: choice }));
    } catch (error) { setError(String(error)); }
  }
  const connectionBlocked = !connection.connected || connection.reconnecting || connection.stale || connection.uncertain;
  const status = connection.stale ? "The conversation changed. Reload the panel." : connection.reconnecting ? "Reconnecting" : !connection.connected ? "Connection lost" : connection.uncertain ? "Check saved work before starting again" : connection.error || failure || error || navigation.error || (ready ? capturing ? "Reading current page" : approvalPending || browserApprovalPending ? "Waiting for approval" : agent.isRunning || submitting || nativeRunning ? "Running" : reading ? "Loading history" : "Ready" : "Connecting");
  const controlsDisabled = connectionBlocked || navigation.navigationDisabled || !ready || capturing || reading || submitting || nativeRunning || agent.isRunning;
  const prompt = (<><div data-pi-composer-slot="" ref={setPiComposerSlot} /><PromptBar draft={text} onDraftChange={value => { draftRevision.current += 1; setText(value); }}
      choices={choices} selection={selection} onSelect={selectExecution} codexMode={codexMode} onCodexMode={setCodexMode} onSend={submit} onStop={() => {
        if (recovery && nativeRunning && !agent.isRunning) {
          if (recoveryStopPending.current) return;
          recoveryStopPending.current = true;
          void (async () => { await recovery.stop(); })()
            .catch(error => { if (mounted.current) setError(`Stop was not confirmed: ${String(error)}`); })
            .finally(() => { recoveryStopPending.current = false; });
        } else agent.abortRun();
      }}
      onCapture={captureCurrentPage} disabled={controlsDisabled}
      running={agent.isRunning || nativeRunning} captureDisabled={controlsDisabled} /></>);
  const toolbar = <div className="chat-toolbar">
    <span data-bridge-status="" role="status">{status}</span>
    <ConversationHistory bridge={bridge} enqueue={enqueue} disabled={controlsDisabled} onSelect={navigation.onSelect} />
    <button type="button" onClick={navigation.openSettings}>Settings</button>
    {!connection.connected && <button id="bridge-reconnect" type="button" disabled={!connection.can_reconnect} onClick={connection.reconnect}>Reconnect</button>}
    {(connection.uncertain || connection.stale) && <button id="bridge-review-saved" type="button" disabled={connection.reconnecting} onClick={review}>Review saved work</button>}
    {reviewError && <span id="bridge-review-error" role="alert">{reviewError}</span>}
    {navigation.error && <button type="button" onClick={navigation.retry}>Try again</button>}
    <button id="bridge-older" disabled={capturing || reading || !archive.olderCursor} type="button" onClick={() => {
      const cursor = archive.olderCursor;
      if (!cursor) return;
      setError("");
      void readArchive(async () => {
        const encoded = await bridge.load_older(cursor);
        const data: unknown = JSON.parse(encoded);
        if (mounted.current) setArchive(ArchivePage.parse(data));
      }).catch(error => { if (mounted.current) setError(String(error)); });
    }}>Older messages</button>
    <button id="bridge-latest" disabled={capturing || reading || submitting || agent.isRunning} type="button" onClick={() => {
      setError("");
      void readArchive(latest).catch(error => { if (mounted.current) setError(String(error)); });
    }}>Latest messages</button>

  </div>;
  return <ChatLayout {...navigation} navigationDisabled={controlsDisabled}
    status={null} composer={prompt} content={visible.length ? {kind:"thread", body:<>
      {toolbar}
      <SavedSource bridge={bridge} />
      <div data-bridge-messages=""><MessageThread messages={visible} liveMessageIds={liveIds} running={agent.isRunning || submitting || nativeRunning} renderActivity={renderActivityMessage} /></div>
      <NativePermissions bridge={bridge} onPendingChange={setApprovalPending} />
      <BrowserActions bridge={bridge} onPendingChange={setBrowserApprovalPending} />
      <CodexInput bridge={bridge} />
      <PiUi bridge={bridge} destination={selection.value} draft={text} onApply={applyPiSuggestion} composerSlot={piComposerSlot} />
    </>} : {kind:"empty", body:<div className="chat-empty">
      <h1><span>Welcome to metldr</span><span>What do you want to do?</span></h1>
      {toolbar}
      <SavedSource bridge={bridge} />
      <NativePermissions bridge={bridge} onPendingChange={setApprovalPending} />
      <BrowserActions bridge={bridge} onPendingChange={setBrowserApprovalPending} />
      <CodexInput bridge={bridge} />
      <PiUi bridge={bridge} destination={selection.value} draft={text} onApply={applyPiSuggestion} composerSlot={piComposerSlot} />
    </div>}} />;
}

function App({ initial, destination, bridge, recovery, initialChoices, settings, agentSettings, preferences, capture, navigation, enqueue, onSessionMounted, onHistoryChange }: { initial: z.infer<typeof Snapshot>; destination: string; bridge: UiBridge; recovery?: ReturnType<UiBridge["active_run"]>; initialChoices: PromptChoice[]; settings: UiProviderSettings; agentSettings: UiAgentSettings; preferences: UiPreferences; capture: Capture; navigation: Omit<NavigationControls,"openSettings">; enqueue: Enqueue; onSessionMounted(): void; onHistoryChange(): Promise<void> }) {
  useEffect(onSessionMounted, [bridge, onSessionMounted]);
  const [failure, setFailure] = useState("");
  const settingsDialog = useRef<HTMLDialogElement>(null);
  const preferencesView = usePreferences(preferences);
  const mounted = useRef(false);
  const [controlRevision, setControlRevision] = useState(0);
  const controlsChanged = useCallback(() => setControlRevision(value => value + 1), []);
  const readTable = useCallback((reference: z.infer<typeof TableReference>) =>
    enqueue(() => bridge.table(reference.runId, reference.artifactId)), [bridge, enqueue]);
  const readNativeTools = useCallback((reference: string) =>
    enqueue(() => bridge.native_tools(reference)), [bridge, enqueue]);
  const readRunDetails = useCallback((reference: string) =>
    enqueue(() => bridge.run_details(reference)), [bridge, enqueue]);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const agents = useMemo(() => ({ default: new RustAgent({
    destination, mode: "configured", capture,
    bridgeFactory: async () => bridge,
    recovery,
    onStopFailure: error => { if (mounted.current) setFailure(`Stop was not confirmed: ${String(error)}`); },
  }) }), [bridge, destination]);
  const renderActivityMessages = useMemo(() => [{
    activityType: "metldr.table",
    content: TableReference,
    render: ({ content, agent }: { content: z.infer<typeof TableReference>; agent: unknown }) => {
      if (!(agent instanceof RustAgent) || agent === agents.default || agent.threadId !== initial.threadId) {
        throw new Error("The table belongs to another conversation.");
      }
      return <SavedTable reference={content} read={readTable} />;
    },
  }, {
    activityType: "metldr.native-tools",
    content: z.unknown(),
    render: ({ content, agent }: { content: unknown; agent: unknown }) => {
      if (!(agent instanceof RustAgent) || agent === agents.default || agent.threadId !== initial.threadId) {
        throw new Error("The tool activity belongs to another conversation.");
      }
      return <NativeToolChips reference={content} read={readNativeTools} />;
    },
  }, {
    activityType: "metldr.run-details",
    content: z.unknown(),
    render: ({ content, agent }: { content: unknown; agent: unknown }) => {
      if (!(agent instanceof RustAgent) || agent === agents.default || agent.threadId !== initial.threadId) {
        throw new Error("The run details belong to another conversation.");
      }
      return <RunDetails reference={content} read={readRunDetails} />;
    },
  }], [agents, initial.threadId, readTable, readNativeTools, readRunDetails]);
  return <><InteractionSounds enabled={preferencesView.sounds_enabled} /><CopilotKitProvider key={initial.threadId} selfManagedAgents={agents} renderActivityMessages={renderActivityMessages} showDevConsole={false}>
    <CopilotChatConfigurationProvider threadId={initial.threadId}>
      <Conversation initial={initial} failure={failure} bridge={bridge} recovery={recovery} enqueue={enqueue} destination={destination} initialChoices={initialChoices} controlRevision={controlRevision} onHistoryChange={onHistoryChange} navigation={{...navigation, openSettings: () => settingsDialog.current?.showModal()}} />

    </CopilotChatConfigurationProvider>
  </CopilotKitProvider>
    <dialog className="chat-settings" ref={settingsDialog}>
      <header><h2>Settings</h2><button type="button" onClick={() => settingsDialog.current?.close()}>Close</button></header>
      <PreferenceSettings preferences={preferences} view={preferencesView} />
      <ProviderSettings settings={settings} onConnectionsChange={controlsChanged} />
      <AgentSettings settings={agentSettings} onConfigurationsChange={controlsChanged} />
    </dialog></>;
}

type Session = {
  bridge: UiBridge;
  initial: z.infer<typeof Snapshot>;
  choices: PromptChoice[];
  preferred: ReturnType<UiBridge["preferred_execution_choice"]>;
  recovery: ReturnType<UiBridge["active_run"]>;
};
type Navigation =
  | { kind: "idle" }
  | { kind: "titles-error"; error: string }
  | { kind: "navigation-error"; target: string; error: string }
  | { kind: "pending" }
  | { kind: "retry"; target: string; error: string }
  | { kind: "selected"; bridge: UiBridge; target: string; error: string };

function canNavigate(navigation: Navigation) {
  return navigation.kind === "idle" || navigation.kind === "titles-error" || navigation.kind === "navigation-error";
}

async function readSession(bridge: UiBridge): Promise<Session> {
  const raw: unknown = JSON.parse(await bridge.snapshot());
  const initial = Snapshot.parse(raw);
  const choices = await executionChoices(bridge);
  return { bridge, initial, choices, preferred: bridge.preferred_execution_choice(), recovery: initial.running ? bridge.active_run() : undefined };
}

function Startup({ preferences, error }: { preferences: UiPreferences; error?: string }) {
  const view = usePreferences(preferences);
  return <section className="chat-startup">
    <InteractionSounds enabled={view.sounds_enabled} />
    <p role={error ? "alert" : "status"}>{error ? `metldr could not connect. ${error}` : "Opening metldr"}</p>
    {error && <button type="button" onClick={() => location.reload()}>Reload</button>}
    <PreferenceSettings preferences={preferences} view={view} />
  </section>;
}

function release(session: Session) {
  session.recovery?.detach();
  session.recovery?.free();
  session.bridge.free();
}

export async function mountChat(container: HTMLElement, { capture, destination = "model" }: { capture: Capture; destination?: string }) {
  await init();
  const preferences = new UiPreferences();
  container.dataset.metldrChat = "";
  const root = createRoot(container);
  root.render(<Startup preferences={preferences} />);
  let bridge: UiBridge | undefined;
  let current: Session;
  try {
    bridge = await connect_ui_bridge(capture);
    current = await readSession(bridge);
  } catch (error) {
    bridge?.free();
    root.render(<Startup preferences={preferences} error={String(error)} />);
    let closed = false;
    return {
      thread: "", sourceAvailable: false,
      unmount() {
        if (closed) return;
        closed = true;
        root.unmount();
        preferences.close();
        preferences.free();
      },
    };
  }
  const settings = bridge.provider_settings();
  const agentSettings = bridge.agent_settings();
  let closed = false;
  let navigation: Navigation = { kind: "idle" };
  let readTail = Promise.resolve();
  const enqueue: Enqueue = <T,>(operation: () => Promise<T>): Promise<T> => {
    const owner = current;
    const next = readTail.then(() => {
      if (closed || current !== owner) throw new Error("The conversation was closed.");
      return operation();
    });
    readTail = next.then(() => {}, () => {});
    return next;
  };
  let tabs = [{ id: current.initial.threadId, title: "New chat" }];
  let retired: Session[] = [];
  let titlesPending: Session | undefined;

  function retire() {
    const previous = retired;
    retired = [];
    previous.forEach(release);
  }

  async function refreshTitles() {
    const owner = current;
    if (titlesPending === owner) return;
    titlesPending = owner;
    try {
      await enqueue(async () => {
        const encoded: unknown = JSON.parse(await owner.bridge.conversations());
        const page = ConversationPage.parse(encoded);
        if (closed || current !== owner) return;
        tabs = tabs.map(tab => {
          const saved = page.conversations.find(row => row.id === tab.id);
          return saved?.title.text ? {...tab,title:saved.title.text} : tab;
        });
        if (navigation.kind === "titles-error") navigation = {kind:"idle"};
      });
    } catch (error) {
      if (closed || current !== owner) return;
      if (navigation.kind === "idle" || navigation.kind === "titles-error") {
        navigation = {kind:"titles-error", error:String(error)};
      }
    } finally {
      if (titlesPending === owner) titlesPending = undefined;
    }
    render();
  }

  function retry() {
    switch (navigation.kind) {
      case "titles-error": void refreshTitles(); break;
      case "navigation-error": void navigate(navigation.target); break;
      case "retry": case "selected": void navigate("retry"); break;
      case "idle": case "pending": break;
    }
  }

  function render() {
    if (closed) return;
    root.render(<App initial={current.initial} destination={current.preferred ?? destination} bridge={current.bridge}
      recovery={current.recovery} initialChoices={current.choices} settings={settings} agentSettings={agentSettings} preferences={preferences}
      capture={capture} enqueue={enqueue} onSessionMounted={retire} onHistoryChange={refreshTitles} navigation={{ tabs, activeId: current.initial.threadId,
        navigationDisabled: !canNavigate(navigation),
        error: navigation.kind === "pending" ? "Opening conversation" : navigation.kind === "idle" ? "" : navigation.error,
        onNew: () => { void navigate("new"); },
        onSelect: id => { if (id !== current.initial.threadId) void navigate(id); },
        onClose: id => {
          if (!canNavigate(navigation)) return;
          if (id !== current.initial.threadId) { tabs = tabs.filter(tab => tab.id !== id); render(); return; }
          const remaining = tabs.filter(tab => tab.id !== id);
          void navigate(remaining.at(-1)?.id ?? "new", id);
        }, retry,
      }} />);
  }

  async function navigate(target: string, closing?: string) {
    if (closed || navigation.kind === "pending") return;
    if (target !== "retry" && !canNavigate(navigation)) return;
    const previousNavigation = navigation;
    const retryTarget = target === "retry" && "target" in previousNavigation ? previousNavigation.target : target;
    navigation = { kind: "pending" };
    render();
    let next: UiBridge;
    try {
      next = await enqueue(async () => previousNavigation.kind === "selected" ? previousNavigation.bridge
        : target === "retry" ? await current.bridge.retry_navigation()
        : target === "new" ? await current.bridge.new_conversation()
        : await current.bridge.select_conversation(target));
    } catch (error) {
      if (closed) return;
      const message = String(error);
      navigation = current.bridge.navigation_pending ? { kind: "retry", target: retryTarget, error: message } : { kind: "navigation-error", target: retryTarget, error: message };
      render();
      return;
    }
    if (closed) { next.free(); return; }
    let session: Session;
    try { session = await readSession(next); }
    catch (error) {
      if (closed) { next.free(); return; }
      navigation = { kind: "selected", bridge: next, target: retryTarget, error: String(error) };
      render();
      return;
    }
    if (closed) { release(session); return; }
    retired.push(current);
    current = session;
    tabs = tabs.filter(tab => tab.id !== closing);
    if (!tabs.some(tab => tab.id === session.initial.threadId)) tabs = [...tabs, { id: session.initial.threadId, title: "New chat" }];
    navigation = { kind: "idle" };
    render();
    void refreshTitles();
  }

  render();
  void refreshTitles();
  return {
    get thread() { return current.initial.threadId; },
    get sourceAvailable() { return current.initial.sourceAvailable; },
    unmount() {
      if (closed) return;
      closed = true;
      root.unmount();
      if (navigation.kind === "selected") navigation.bridge.free();
      for (const previous of retired) release(previous);
      retired = [];
      release(current);
      settings.free();
      agentSettings.free();
      preferences.close();
      preferences.free();
    },
  };
}
