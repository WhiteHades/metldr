export default createSession => {
  const post = JSON.stringify;
  const loadScript = importScripts.bind(globalThis);
  const evaluate = globalThis.eval;
  const later = queueMicrotask.bind(globalThis);
  const createUrl = URL.createObjectURL.bind(URL);
  const revokeUrl = URL.revokeObjectURL.bind(URL);
  const SourceBlob = Blob;
  const resolve = Promise.resolve.bind(Promise);
  const then = Function.prototype.call.bind(Promise.prototype.then);
  const readMessage = Function.prototype.call.bind(Object.getOwnPropertyDescriptor(MessageEvent.prototype, "data").get);
  let initialized = false;

  globalThis.onmessage = event => {
    const initial = readMessage(event);
    if (initialized || !initial || !(initial.port instanceof MessagePort)) return;
    initialized = true;
    globalThis.onmessage = null;
    const port = initial.port;
    const send = port.postMessage.bind(port);
    const { initialization, module, documentId, nodes: snapshot, scripts, followup, taskLimit } = initial;
    const session = createSession(initialization, module);
    const { binding } = JSON.parse(initialization);
    let sequence = 0;
    let operations = [];
    let running = false;
    let scheduled = false;
    let lastRequest = 1;
    let followupPending = false;
    let listenerCount = 0;
    let nextTask = 0;
    let pendingTasks = 0;
    const task = (kind, id) => send(post({ binding, documentId, task: { kind, id } }));
    const nodes = snapshot.map((data, index) => ({ data, id: index + 1, alive: true, listeners: new Map() }));
    const failed = () => send(session.failed(lastRequest));
    const flush = () => {
      if (!operations.length) return;
      const encoded = post({ binding, documentId, sequence: sequence + 1, operations });
      if (encoded.length > 262144) throw new Error("Answer changes exceed their limit.");
      send(encoded);
      sequence++;
      operations = [];
    };
    const change = operation => {
      if (operations.length >= 256) throw new Error("Answer has too many pending changes.");
      operations.push(operation);
      if (!scheduled) {
        scheduled = true;
        later(() => {
          scheduled = false;
          if (!running) { try { flush(); } catch { failed(); } }
        });
      }
    };
    const active = state => {
      if (!state.alive) throw new Error("Answer element was removed.");
      return state;
    };
    const listen = (state, kind, callback) => {
      active(state);
      if ((kind !== "input" && kind !== "click") || typeof callback !== "function") {
        throw new Error("Answer event is not supported.");
      }
      let callbacks = state.listeners.get(kind);
      if (!callbacks) {
        callbacks = new Set();
        state.listeners.set(kind, callbacks);
        change({ kind: "listen", node: state.id, event: kind });
      }
      if (!callbacks.has(callback)) {
        if (listenerCount >= 8192) throw new Error("Answer has too many listeners.");
        callbacks.add(callback);
        listenerCount++;
      }
    };
    const track = value => {
      if (value === null || (typeof value !== "object" && typeof value !== "function") || typeof value.then !== "function") return;
      if (pendingTasks >= taskLimit || nextTask >= 0xffffffff) throw new Error("Answer task limit reached.");
      const id = ++nextTask;
      pendingTasks++;
      task("started", id);
      const settle = kind => {
        pendingTasks--;
        try { flush(); task(kind, id); } catch { failed(); }
      };
      then(resolve(value), () => settle("finished"), () => settle("failed"));
    };
    const dispatch = (state, kind, trusted) => {
      const target = elements[active(state).id - 1];
      const event = { type: kind, isTrusted: trusted, target, currentTarget: target, preventDefault() {}, stopPropagation() {} };
      for (const callback of state.listeners.get(kind) ?? []) track(callback.call(target, event));
    };
    class Element {
      constructor(state) {
        this.state = state;
        this.dataset = new Proxy(Object.assign(Object.create(null), state.data.dataset), {
          set(target, key, value) {
            active(state);
            if (typeof key !== "string") throw new Error("Invalid answer data key.");
            const text = String(value);
            change({ kind: "setDataset", node: state.id, key, value: text });
            target[key] = text;
            return true;
          },
        });
      }
      get id() { return this.state.data.id; }
      get tagName() { return this.state.data.tag; }
      get value() { return active(this.state).data.value; }
      set value(value) {
        const state = active(this.state), text = String(value);
        change({ kind: "setValue", node: state.id, value: text });
        state.data.value = text;
      }
      get disabled() { return active(this.state).data.disabled; }
      set disabled(value) {
        const state = active(this.state), disabled = Boolean(value);
        change({ kind: "setDisabled", node: state.id, disabled });
        state.data.disabled = disabled;
      }
      get textContent() {
        return active(this.state).data.children.map(child => typeof child === "string" ? child : elements[child - 1].textContent).join("");
      }
      set textContent(value) {
        const state = active(this.state), text = String(value ?? "");
        change({ kind: "setText", node: state.id, text });
        state.data.children = [text];
        for (let index = state.id; index < nodes.length; index++) {
          const child = nodes[index], parent = child.data.parent;
          if (parent === state.id || (parent && !nodes[parent - 1].alive)) {
            child.alive = false;
            for (const callbacks of child.listeners.values()) listenerCount -= callbacks.size;
            child.listeners.clear();
          }
        }
      }
      addEventListener(kind, callback) { listen(this.state, kind, callback); }
      removeEventListener(kind, callback) {
        if (this.state.listeners.get(kind)?.delete(callback)) listenerCount--;
      }
      click() { dispatch(this.state, "click", false); }
    }
    const elements = nodes.map(state => new Element(state));
    const document = Object.freeze({
      body: elements[0],
      getElementById(id) {
        const node = nodes.find(node => node.alive && node.data.id === String(id));
        return node ? elements[node.id - 1] : null;
      },
    });
    Object.defineProperties(globalThis, {
      window: { value: globalThis, writable: false, configurable: false },
      document: { value: document, writable: false, configurable: false },
      sendPrompt: { value: text => {
        if (followup && !followupPending && typeof text === "string") {
          const encoded = session.followup(text);
          if (encoded !== undefined) { followupPending = true; send(encoded); }
        }
      }, writable: false, configurable: false },
    });
    const script = code => {
      const url = createUrl(new SourceBlob([code], { type: "text/javascript" }));
      try { loadScript(url); } finally { revokeUrl(url); }
    };
    port.onmessage = async event => {
      const command = readMessage(event);
      if (command.kind === "probe") { task("pong", command.id); return; }
      if (running) { failed(); return; }
      running = true;
      lastRequest = command.requestId;
      try {
        switch (command.kind) {
          case "document": for (const code of scripts) script(code); break;
          case "functions": script(command.code); break;
          case "expression": await evaluate(command.code); break;
          case "consume": followupPending = false; break;
          case "event": {
            const input = JSON.parse(command.encoded);
            const state = active(nodes[input.node - 1]);
            if (input.kind === "input") state.data.value = input.value;
            dispatch(state, input.kind, true);
            break;
          }
          default: throw new Error("Invalid answer command.");
        }
        flush();
        send(session.ack(lastRequest));
      } catch { failed(); }
      finally { running = false; }
    };
    port.start();
  };
};
