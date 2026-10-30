import { createContext, useContext, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { AnswerEventKind, AnswerFailureKind, type UiAnswerFrame } from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./generated-answer.css";

export const GeneratedReference = z.object({
  runId: z.string().min(1).max(128),
  artifactId: z.string().min(1).max(128), revision: z.string().regex(/^[1-9][0-9]{0,19}$/),
  completion: z.enum(["streaming", "completed", "interrupted"]),
}).strict();
type Reference = z.infer<typeof GeneratedReference>;
type Enqueue = <T>(operation: () => Promise<T>) => Promise<T>;
export const AnswerDraft = createContext<(text: string) => void>(() => {});

function sandboxUrl() {
  const url = new URL("/answer-sandbox.html", window.location.href).href;
  if (!document.querySelector("meta[data-answer-frame-policy]")) {
    const policy = document.createElement("meta");
    policy.httpEquiv = "Content-Security-Policy";
    policy.content = `frame-src ${url}`;
    policy.dataset.answerFramePolicy = "";
    document.head.append(policy);
  }
  return url;
}

export function GeneratedAnswer({ reference, open, enqueue }: {
  reference: Reference; open(reference: Reference): UiAnswerFrame; enqueue: Enqueue;
}) {
  const target = useRef(reference);
  target.current = reference;
  const container = useRef<HTMLDivElement>(null);
  const current = useRef<{ retry(): void; export(): Promise<void>; consume(): boolean; pause(): void; refresh(): Promise<void> } | null>(null);
  const [height, setHeight] = useState(200);
  const [visible, setVisible] = useState(false);
  const [failure, setFailure] = useState("");
  const [proposal, setProposal] = useState("");
  const [exporting, setExporting] = useState(false);
  const [saveFailure, setSaveFailure] = useState("");
  const addDraft = useContext(AnswerDraft);
  const { runId, artifactId } = reference;

  useEffect(() => {
    const owner = container.current;
    if (!owner) return;
    let answer: UiAnswerFrame;
    try { answer = open(target.current); }
    catch { setFailure("The answer could not be opened."); return; }
    let disposed = false;
    let reading = false;
    let saving = false;
    let freed = false;
    let loaded = "";
    let completion = "streaming";
    let transport: { iframe: HTMLIFrameElement; port: MessagePort; timer?: ReturnType<typeof setTimeout> } | undefined;
    setExporting(false);
    setSaveFailure("");
    const free = () => { if (disposed && !reading && !saving && !freed) { freed = true; answer.free(); } };
    const destroy = () => {
      const previous = transport;
      transport = undefined;
      if (!previous) return;
      clearTimeout(previous.timer);
      previous.port.close();
      previous.iframe.remove();
    };
    const fail = (message: string) => { destroy(); if (!disposed) { setProposal(""); setFailure(message); } };
    function pump() {
      if (!transport || disposed) return;
      const encoded = answer.next_command();
      if (encoded === undefined) return;
      clearTimeout(transport.timer);
      transport.timer = setTimeout(() => fail("The answer stopped responding. Retry to reopen it."), 5000);
      transport.port.postMessage(encoded);
    }
    const attach = () => {
      destroy();
      setProposal("");
      const iframe = document.createElement("iframe");
      iframe.title = "Interactive answer";
      iframe.sandbox.add("allow-scripts");
      iframe.referrerPolicy = "no-referrer";
      const channel = new MessageChannel();
      const attached: NonNullable<typeof transport> = { iframe, port: channel.port1 };
      transport = attached;
      attached.timer = setTimeout(() => fail("The answer could not be opened."), 5000);
      attached.port.onmessage = (event: MessageEvent<unknown>) => {
        if (disposed || transport !== attached) return;
        try {
          if (typeof event.data !== "string") throw new Error("Invalid answer message.");
          const incoming = answer.receive(event.data);
          try {
            const kind = incoming.kind();
            switch (kind) {
            case AnswerEventKind.Height: setHeight(incoming.height()); break;
            case AnswerEventKind.Followup: setProposal(incoming.text()); break;
            case AnswerEventKind.Failed: fail("The answer could not finish its display. Retry to reopen it."); return;
            case AnswerEventKind.RuntimeFailed:
              fail(incoming.failure_kind() === AnswerFailureKind.Timeout
                ? "The answer stopped responding. Retry to reopen it."
                : "The answer could not finish its display. Retry to reopen it.");
              return;
            case AnswerEventKind.Ready: case AnswerEventKind.Ack: clearTimeout(attached.timer); break;
            default: { const exhaustive: never = kind; throw new Error(String(exhaustive)); }
            }
            pump();
          } finally { incoming.free(); }
        } catch { fail("The answer sent an invalid display update."); }
      };
      attached.port.start();
      iframe.addEventListener("load", () => {
        if (disposed || transport !== attached) return;
        iframe.contentWindow?.postMessage(answer.initialize(), "*", [channel.port2]);
      }, { once: true });
      iframe.src = sandboxUrl();
      owner.append(iframe);
    };
    async function refresh() {
      if (reading || disposed) return;
      reading = true;
      try {
        while (!disposed && target.current.runId === runId && target.current.artifactId === artifactId && target.current.revision !== loaded) {
          const wanted = target.current;
          let refreshed: boolean;
          try { refreshed = await enqueue(() => answer.refresh(wanted.revision)); }
          catch {
            if (target.current.revision !== wanted.revision) continue;
            fail("The saved answer could not be loaded. Retry to reopen it.");
            return;
          }
          if (disposed) return;
          if (!refreshed) {
            const next = target.current;
            if (next.runId !== wanted.runId || next.artifactId !== wanted.artifactId || next.revision !== wanted.revision || next.completion !== wanted.completion) continue;
            return;
          }
          if (!loaded) setHeight(answer.height());
          loaded = wanted.revision;
          const nextCompletion = answer.completion();
          if (nextCompletion === "interrupted" && completion !== "interrupted") {
            answer.restart();
            attach();
          }
          completion = nextCompletion;
          setVisible(answer.has_content());
          pump();
        }
      } catch { fail("The answer could not be displayed."); }
      finally { reading = false; free(); }
    }
    function retry() {
      if (reading || disposed) return;
      try {
        setFailure("");
        loaded = "";
        answer.restart();
        attach();
        void refresh();
      } catch { fail("The answer could not be reopened."); }
    }
    current.current = {
      retry,
      pause: destroy,
      refresh,
      consume() {
        try { answer.consume_followup(); pump(); return true; }
        catch { fail("The followup could not be dismissed. Retry to reopen the answer."); return false; }
      },
      async export() {
        if (saving || disposed) return;
        saving = true;
        setExporting(true);
        setSaveFailure("");
        try {
          const html = await answer.export_html();
          if (disposed || html === undefined) return;
          const url = URL.createObjectURL(new Blob([html], { type: "text/html;charset=utf-8" }));
          const link = document.createElement("a");
          link.href = url; link.download = "metldr-answer.html";
          try { link.click(); } finally { setTimeout(() => URL.revokeObjectURL(url), 0); }
        } catch { if (!disposed) setSaveFailure("The answer could not be saved."); }
        finally { saving = false; if (!disposed) setExporting(false); free(); }
      },
    };
    attach();
    void refresh();
    return () => {
      disposed = true;
      current.current = null;
      destroy();
      answer.close();
      free();
    };
  }, [open, enqueue, runId, artifactId]);

  useEffect(() => {
    if (reference.completion === "interrupted") current.current?.pause();
    void current.current?.refresh();
  }, [reference.revision, reference.completion]);

  return <section className="generated-answer" data-generated-answer="" aria-label="Generated answer">
    <div className="generated-answer-stage" data-visible={visible} hidden={Boolean(failure)}>
      <div className="generated-answer-frame" ref={container} style={{ blockSize: `${height}px` }} />
      {!visible && !failure && <p className="generated-answer-loading" role="status">{reference.completion === "streaming" ? "Creating answer" : "The saved answer is unfinished."}</p>}
    </div>
    {failure && <p role="alert">{failure}</p>}
    {saveFailure && <p role="alert">{saveFailure}</p>}
    <div className="generated-answer-actions">
      {failure && <button type="button" onClick={() => current.current?.retry()}>Retry answer</button>}
      {reference.completion === "completed" && <button type="button" disabled={exporting} aria-busy={exporting} onClick={event => {
        if (event.isTrusted) void current.current?.export();
      }}>{exporting ? "Saving answer" : "Save answer"}</button>}
      {reference.completion === "interrupted" && <span>Generation stopped</span>}
    </div>
    {proposal && <div className="generated-answer-followup">
      <p>{proposal}</p>
      <button type="button" onClick={event => {
        if (!event.isTrusted || !current.current?.consume()) return;
        addDraft(proposal);
        setProposal("");
      }}>Add to draft</button>
      <button type="button" onClick={event => {
        if (event.isTrusted && current.current?.consume()) setProposal("");
      }}>Dismiss</button>
    </div>}
  </section>;
}
