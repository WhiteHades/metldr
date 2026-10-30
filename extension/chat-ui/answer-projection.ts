import { AnswerEventKind, AnswerFailureKind, DomEventKind, DomMutationKind, TaskEventKind, type DomSession, type GuestSession } from "../../crates/metldr-answer-codec/pkg/answer_codec.js";
import workerSource from "./answer-worker-entry.ts?worker-source";

type Work = { kind: "document" | "consume" } | { kind: "functions" | "expression"; code: string }
  | { kind: "event"; node: number; event: DomEventKind; encoded: string };
type Job = { work: Work; resolve(): void; reject(error: Error): void };

export class AnswerProjection {
  private readonly worker: Worker;
  private readonly port: MessagePort;
  private readonly url: string;
  private readonly dom: DomSession;
  private readonly elements: Element[];
  private readonly abort = new AbortController();
  private readonly listeners = new Set<string>();
  private readonly queue: Job[] = [];
  private pending: (Job & { id: number; timer: ReturnType<typeof setTimeout> }) | undefined;
  private nextRequest = 0;
  private closed = false;
  private health: { id: number; timer: ReturnType<typeof setTimeout> } | undefined;
  private nextProbe: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly options: {
    session: GuestSession; initialization: string; module: WebAssembly.Module; documentId: number;
    scripts: string[]; followup: boolean; onFollowup(text: string): void; onFailure(reason: AnswerFailureKind): void;
  }) {
    this.elements = [document.body, ...document.body.querySelectorAll("*")];
    const ids = new Map(this.elements.map((element, index) => [element, index + 1]));
    const nodes = this.elements.map(element => ({
      parent: element.parentElement ? ids.get(element.parentElement) ?? 0 : 0,
      id: element.id, tag: element.tagName,
      children: Array.from(element.childNodes).flatMap<string | number>(node => {
        if (node instanceof Text) return [node.data];
        const id = node instanceof Element ? ids.get(node) : undefined;
        return id === undefined ? [] : [id];
      }),
      value: "value" in element && typeof element.value === "string" ? element.value : undefined,
      disabled: "disabled" in element && typeof element.disabled === "boolean" ? element.disabled : undefined,
      dataset: Object.fromEntries(Array.from(element.attributes)
        .filter(attribute => attribute.name.startsWith("data-"))
        .map(attribute => [attribute.name.slice(5).replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase()), attribute.value])),
    }));
    this.dom = options.session.mount_dom(Uint32Array.from(nodes.map(node => node.parent)));
    this.url = URL.createObjectURL(new Blob([workerSource], { type: "text/javascript" }));
    const channel = new MessageChannel();
    this.port = channel.port1;
    try { this.worker = new Worker(this.url); }
    catch (error) {
      channel.port1.close(); channel.port2.close();
      URL.revokeObjectURL(this.url); this.dom.close(); this.dom.free();
      throw error;
    }
    this.port.onmessage = event => this.receive(event.data);
    this.worker.onerror = event => { event.preventDefault(); this.fail(); };
    try { this.worker.postMessage({
      initialization: options.initialization, module: options.module, documentId: options.documentId,
      nodes, scripts: options.scripts, followup: options.followup, taskLimit: this.dom.task_limit(), port: channel.port2,
    }, [channel.port2]); } catch (error) { this.close(); channel.port2.close(); throw error; }
    this.port.start();
    document.addEventListener("visibilitychange", () => {
      this.stopHealth();
      if (!this.closed) this.watchHealth();
    }, { signal: this.abort.signal });
    this.watchHealth();
  }

  run(work: Work): Promise<void> {
    if (this.closed || this.queue.length >= 32) return Promise.reject(new Error("Answer queue is unavailable."));
    return new Promise((resolve, reject) => { this.queue.push({ work, resolve, reject }); this.pump(); });
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    this.stopHealth();
    this.abort.abort();
    this.worker.terminate();
    this.port.close();
    URL.revokeObjectURL(this.url);
    this.dom.close();
    this.dom.free();
    const pending = this.pending;
    this.pending = undefined;
    if (pending) { clearTimeout(pending.timer); pending.reject(new Error("Answer closed.")); }
    for (const job of this.queue.splice(0)) job.reject(new Error("Answer closed."));
  }

  private fail(reason = AnswerFailureKind.Execution) {
    if (this.closed) return;
    this.close();
    this.options.onFailure(reason);
  }

  private stopHealth() {
    if (this.health) clearTimeout(this.health.timer);
    if (this.nextProbe !== undefined) clearTimeout(this.nextProbe);
    this.health = undefined;
    this.nextProbe = undefined;
  }

  private watchHealth() {
    // Native callbacks can run without a tracked promise after a command completes.
    if (this.closed || this.health || this.nextProbe !== undefined || document.visibilityState !== "visible") return;
    const id = this.dom.probe();
    const timeout = this.dom.probe_timeout_ms();
    const deadline = performance.now() + timeout;
    const timer = setTimeout(() => {
      if (this.closed || this.health?.id !== id) return;
      this.health = undefined;
      if (document.visibilityState !== "visible" || performance.now() - deadline > this.dom.probe_interval_ms()) {
        this.watchHealth();
      } else this.fail(AnswerFailureKind.Timeout);
    }, timeout);
    this.health = { id, timer };
    this.port.postMessage({ kind: "probe", id });
  }

  private task(data: string): boolean {
    let event: ReturnType<DomSession["receive_task"]>;
    try { event = this.dom.receive_task(data); } catch { return false; }
    try {
      const kind = event.kind();
      switch (kind) {
        case TaskEventKind.Started: this.watchHealth(); break;
        case TaskEventKind.Finished: break;
        case TaskEventKind.Failed: this.fail(); break;
        case TaskEventKind.Pong:
          if (this.health?.id === event.id()) {
            clearTimeout(this.health.timer);
            this.health = undefined;
            if (document.visibilityState === "visible") {
              this.nextProbe = setTimeout(() => { this.nextProbe = undefined; this.watchHealth(); }, this.dom.probe_interval_ms());
            }
          }
          break;
        case TaskEventKind.Ignored: break;
        default: { const exhaustive: never = kind; throw new Error(String(exhaustive)); }
      }
    } catch { this.fail(); }
    finally { event.free(); }
    return true;
  }

  private pump() {
    if (this.closed || this.pending) return;
    const job = this.queue.shift();
    if (!job) return;
    const id = ++this.nextRequest;
    if (id > 0xffffffff) { job.reject(new Error("Answer sequence exhausted.")); this.fail(); return; }
    const timer = setTimeout(() => this.fail(AnswerFailureKind.Timeout), 5000);
    this.pending = { ...job, id, timer };
    this.port.postMessage({ ...job.work, requestId: id });
  }

  private receive(data: unknown) {
    if (this.closed) return;
    if (typeof data !== "string") { this.fail(); return; }
    let batch: ReturnType<DomSession["receive"]> | undefined;
    try { batch = this.dom.receive(data); } catch { }
    if (batch) {
      try {
        for (;;) {
          const operation = batch.next();
          if (!operation) break;
          try {
            const node = operation.node();
            const element = this.elements[node - 1];
            if (!element?.isConnected) throw new Error("Answer element is unavailable.");
            const kind = operation.kind();
            switch (kind) {
              case DomMutationKind.SetText: element.textContent = operation.text(); break;
              case DomMutationKind.SetValue:
                if (!("value" in element) || typeof element.value !== "string") throw new Error("Answer element has no value.");
                element.value = operation.text(); break;
              case DomMutationKind.SetDisabled:
                if (!("disabled" in element) || typeof element.disabled !== "boolean") throw new Error("Answer element cannot be disabled.");
                element.disabled = operation.disabled(); break;
              case DomMutationKind.SetDataset:
                element.setAttribute(`data-${operation.key().replace(/[A-Z]/g, letter => `-${letter.toLowerCase()}`)}`, operation.text()); break;
              case DomMutationKind.Listen: this.listen(element, node, operation.event_kind()); break;
              default: { const exhaustive: never = kind; throw new Error(String(exhaustive)); }
            }
          } finally { operation.free(); }
        }
      } catch { this.fail(); }
      finally { batch.free(); }
      return;
    }
    if (this.task(data)) return;
    try {
      const event = this.options.session.worker_event(data);
      try {
        const kind = event.kind();
        if (kind === AnswerEventKind.Followup) { this.options.onFollowup(event.text()); return; }
        if (kind !== AnswerEventKind.Ack || !this.pending || event.request_id() !== this.pending.id) {
          throw new Error("Invalid answer acknowledgement.");
        }
        const pending = this.pending;
        this.pending = undefined;
        clearTimeout(pending.timer);
        pending.resolve();
        this.pump();
      } finally { event.free(); }
    } catch { this.fail(); }
  }

  private listen(element: Element, node: number, kind: DomEventKind) {
    const key = `${node}:${kind}`;
    if (this.listeners.has(key)) return;
    const type = kind === DomEventKind.Input ? "input" : "click";
    element.addEventListener(type, event => {
      if (!event.isTrusted || this.closed) return;
      try {
        const value = kind === DomEventKind.Input && "value" in element && typeof element.value === "string" ? element.value : undefined;
        const encoded = this.dom.event(node, kind, value);
        const work: Work = { kind: "event", node, event: kind, encoded };
        const last = this.queue.at(-1);
        if (kind === DomEventKind.Input && last?.work.kind === "event" && last.work.node === node && last.work.event === kind) {
          last.work = work;
        } else { void this.run(work).catch(() => this.fail()); }
      } catch { this.fail(); }
    }, { signal: this.abort.signal });
    this.listeners.add(key);
  }
}
