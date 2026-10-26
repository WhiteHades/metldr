import { useLayoutEffect, useRef, useState } from "react";
import {
  UiCodexAnswer,
  UiCodexFieldAnswer,
  UiCodexPromptKind,
  UiCodexQuestionAnswer,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";
import type {
  UiBridge,
  UiCodexField,
  UiCodexOption,
  UiCodexQuestion,
  UiCodexRequest,
  UiCodexSnapshot,
} from "../../crates/metldr-panel/pkg/metldr_panel.js";
import "./codex-input.css";

type CodexOwner = ReturnType<UiBridge["codex_input"]>;
type CodexOption = Pick<UiCodexOption, "value" | "label" | "description">;
type CodexQuestion = Pick<UiCodexQuestion, "id" | "header" | "text" | "other"> & { options: CodexOption[] };
type CodexField = Pick<UiCodexField, "name" | "title" | "description" | "min_chars" | "max_chars" | "default_value"> & { choices: CodexOption[] };
type CodexRequest = Pick<UiCodexRequest,
  "run_id" | "request_id" | "enabled" | "is_blocking" | "item" | "server" | "message" | "review"
> & (
  | { kind: "questions"; questions: CodexQuestion[] }
  | { kind: "form"; fields: CodexField[] }
  | { kind: "tool-approval" }
);
type CodexView = Pick<UiCodexSnapshot,
  "connected" | "stale" | "busy" | "uncertain" | "notice" | "run_id"
> & { requests: CodexRequest[] };

function copyQuestion(question: UiCodexQuestion): CodexQuestion {
  const options = question.options();
  try {
    return {
      id: question.id,
      header: question.header,
      text: question.text,
      other: question.other,
      options: options.map(option => ({ value: option.value, label: option.label, description: option.description })),
    };
  } finally {
    options.forEach(option => option.free());
  }
}

function copyField(field: UiCodexField): CodexField {
  const choices = field.choices();
  try {
    return {
      name: field.name,
      title: field.title,
      description: field.description,
      min_chars: field.min_chars,
      max_chars: field.max_chars,
      default_value: field.default_value,
      choices: choices.map(option => ({ value: option.value, label: option.label, description: option.description })),
    };
  } finally {
    choices.forEach(option => option.free());
  }
}

function copyRequest(request: UiCodexRequest): CodexRequest {
  const common = {
    run_id: request.run_id,
    request_id: request.request_id,
    enabled: request.enabled,
    is_blocking: request.is_blocking,
    item: request.item,
    server: request.server,
    message: request.message,
    review: request.review,
  };
  switch (request.kind) {
    case UiCodexPromptKind.Questions: {
      const questions = request.questions();
      try {
        return { ...common, kind: "questions", questions: questions.map(copyQuestion) };
      } finally {
        questions.forEach(question => question.free());
      }
    }
    case UiCodexPromptKind.McpForm: {
      const fields = request.fields();
      try {
        return { ...common, kind: "form", fields: fields.map(copyField) };
      } finally {
        fields.forEach(field => field.free());
      }
    }
    case UiCodexPromptKind.McpToolApproval:
      return { ...common, kind: "tool-approval" };
    default: {
      const exhaustive: never = request.kind;
      return exhaustive;
    }
  }
}

function copySnapshot(snapshot: UiCodexSnapshot): CodexView {
  const requests = snapshot.requests();
  try {
    return {
      connected: snapshot.connected,
      stale: snapshot.stale,
      busy: snapshot.busy,
      uncertain: snapshot.uncertain,
      notice: snapshot.notice,
      run_id: snapshot.run_id,
      requests: requests.map(copyRequest),
    };
  } finally {
    requests.forEach(request => request.free());
    snapshot.free();
  }
}

function requestKey(runId: string, requestId: string) {
  return JSON.stringify([runId, requestId]);
}

function QuestionsCard({ request, disabled, onAnswer }: {
  request: Extract<CodexRequest, { kind: "questions" }>;
  disabled: boolean;
  onAnswer(request: CodexRequest, answer: UiCodexAnswer, event: React.MouseEvent<HTMLButtonElement>): void;
}) {
  const [answers, setAnswers] = useState(() => new Map<string, { values: string[]; otherText: string }>());
  const ready = request.questions.every(question => {
    const answer = answers.get(question.id);
    const otherLength = answer ? [...answer.otherText].length : 0;
    return Boolean(answer && (!question.other || otherLength <= 4096) &&
      (answer.values.length || (question.other && otherLength > 0)));
  });
  const skip = (event: React.MouseEvent<HTMLButtonElement>) => onAnswer(request, UiCodexAnswer.skip(), event);
  const submit = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (!ready) return;
    const values = request.questions.map(question => {
      const draft = answers.get(question.id) ?? { values: [], otherText: "" };
      return new UiCodexQuestionAnswer(question.id, [...draft.values, ...(question.other && draft.otherText.length ? [draft.otherText] : [])]);
    });
    onAnswer(request, UiCodexAnswer.questions(values), event);
  };
  return <form data-part="answer-form" onSubmit={event => event.preventDefault()}>
    {request.questions.map(question => {
      const draft = answers.get(question.id) ?? { values: [], otherText: "" };
      return <fieldset key={question.id} data-part="question">
        <legend>{question.header || "Question"}</legend>
        {question.text && <p data-part="question-text">{question.text}</p>}
        <div data-part="choices">
          {question.options.map((option, index) => <label key={`${index}:${option.value}`} data-part="option">
            <input type="checkbox" data-part="choice-input" name={`q:${question.id}`} value={option.value} checked={draft.values.includes(option.value)} disabled={disabled}
              onChange={event => {
                const checked = event.currentTarget.checked;
                setAnswers(previous => {
                  const current = previous.get(question.id) ?? { values: [], otherText: "" };
                  return new Map(previous).set(question.id, { ...current,
                    values: checked ? [...current.values, option.value] : current.values.filter(value => value !== option.value) });
                });
              }} />
            <span><span>{option.label}</span>{option.description && <small>{option.description}</small>}</span>
          </label>)}
        </div>
        {question.other && <label data-part="field"><span>Other</span><input type="text" name={`other:${question.id}`} data-part="text-input" aria-label={`${question.header || "Question"}: Other`}
          value={draft.otherText} maxLength={8192} disabled={disabled}
          onChange={event => {
            const otherText = event.currentTarget.value;
            setAnswers(previous => new Map(previous).set(question.id, { ...(previous.get(question.id) ?? draft), otherText }));
          }} /></label>}
      </fieldset>;
    })}
    <div data-part="actions">
      <button type="button" data-action="codex-input" data-codex-input-action="skip" data-codex-input-kind="skip" data-variant="quiet" disabled={disabled} onClick={skip}>Skip</button>
      <button type="submit" data-action="codex-input" data-codex-input-kind="accept" data-variant="primary" disabled={disabled || !ready} onClick={submit}>Submit</button>
    </div>
  </form>;
}

function FormCard({ request, disabled, onAnswer }: {
  request: Extract<CodexRequest, { kind: "form" }>;
  disabled: boolean;
  onAnswer(request: CodexRequest, answer: UiCodexAnswer, event: React.MouseEvent<HTMLButtonElement>): void;
}) {
  const [values, setValues] = useState(() => {
    const initial = new Map<string, string>();
    for (const field of request.fields) initial.set(field.name, field.default_value ?? field.choices[0]?.value ?? "");
    return initial;
  });
  const [validation, setValidation] = useState("");
  const send = (event: React.MouseEvent<HTMLButtonElement>) => {
    const invalid = request.fields.find(field => {
      const value = values.get(field.name) ?? "";
      const count = [...value].length;
      return count < field.min_chars || count > field.max_chars ||
        (field.choices.length > 0 && !field.choices.some(choice => choice.value === value));
    });
    if (invalid) {
      setValidation(invalid.choices.length
        ? `Choose an offered value for ${invalid.title || invalid.name}.`
        : `${invalid.title || invalid.name} needs ${invalid.min_chars} to ${invalid.max_chars} characters.`);
      return;
    }
    setValidation("");
    const answers = request.fields.map(field => new UiCodexFieldAnswer(field.name, values.get(field.name) ?? ""));
    onAnswer(request, UiCodexAnswer.form(answers), event);
  };
  return <form data-part="answer-form" onSubmit={event => event.preventDefault()}>
    {request.fields.map(field => <label key={field.name} data-part="field">
      <span>{field.title || field.name}</span>
      {field.description && <small>{field.description}</small>}
      {field.choices.length ? <select name={`f:${field.name}`} value={values.get(field.name) ?? ""} disabled={disabled}
        onChange={event => { const value = event.currentTarget.value; setValues(previous => new Map(previous).set(field.name, value)); }}>
        {field.choices.map((choice, index) => <option key={`${index}:${choice.value}`} value={choice.value}>{choice.label}</option>)}
      </select> : <input type="text" data-part="text-input" name={`f:${field.name}`} value={values.get(field.name) ?? ""} maxLength={field.max_chars * 2} disabled={disabled}
        onChange={event => { const value = event.currentTarget.value; setValues(previous => new Map(previous).set(field.name, value)); }} />}
    </label>)}
    {validation && <p role="alert">{validation}</p>}
    <div data-part="actions">
      <button type="button" data-action="codex-input" data-codex-input-kind="decline" data-variant="quiet" disabled={disabled}
        onClick={event => onAnswer(request, UiCodexAnswer.deny(), event)}>Decline</button>
      <button type="button" data-action="codex-input" data-codex-input-kind="cancel" data-variant="quiet" disabled={disabled}
        onClick={event => onAnswer(request, UiCodexAnswer.cancel(), event)}>Cancel</button>
      <button type="submit" data-action="codex-input" data-codex-input-kind="accept" data-variant="primary" disabled={disabled} onClick={send}>Accept</button>
    </div>
  </form>;
}

function ToolApprovalCard({ request, titleId, disabled, onAnswer }: {
  request: Extract<CodexRequest, { kind: "tool-approval" }>;
  titleId: string;
  disabled: boolean;
  onAnswer(request: CodexRequest, answer: UiCodexAnswer, event: React.MouseEvent<HTMLButtonElement>): void;
}) {
  return <div data-part="answer-form">
    <div data-part="review" role="group" aria-labelledby={titleId}>
      {request.review && <pre>{request.review}</pre>}
    </div>
    <div data-part="actions">
      <button type="button" data-action="codex-input" data-codex-input-kind="decline" data-variant="quiet" disabled={disabled}
        onClick={event => onAnswer(request, UiCodexAnswer.deny(), event)}>Deny</button>
      <button type="button" data-action="codex-input" data-codex-input-kind="cancel" data-variant="quiet" disabled={disabled}
        onClick={event => onAnswer(request, UiCodexAnswer.cancel(), event)}>Cancel</button>
      <button type="button" data-action="codex-input" data-codex-input-kind="accept" data-variant="primary" disabled={disabled}
        onClick={event => onAnswer(request, UiCodexAnswer.allow(), event)}>Allow</button>
    </div>
  </div>;
}

function CodexRequestCard({ request, disabled, onAnswer }: {
  request: CodexRequest;
  disabled: boolean;
  onAnswer(request: CodexRequest, answer: UiCodexAnswer, event: React.MouseEvent<HTMLButtonElement>): void;
}) {
  const card = useRef<HTMLElement | null>(null);
  const titleId = `codex-input-title-${encodeURIComponent(request.run_id)}-${encodeURIComponent(request.request_id)}`;
  useLayoutEffect(() => {
    if (document.activeElement instanceof Element && document.activeElement.closest("[data-codex-card]")) return;
    card.current?.focus();
  }, []);

  return <article ref={card} role="group" tabIndex={-1} aria-labelledby={titleId} data-codex-run={request.run_id} data-codex-request={request.request_id} data-codex-card>
    {request.kind === "questions" && <>
      <h3 id={titleId}>Questions</h3>
      {!request.is_blocking && <p data-part="message">Codex may continue without a response.</p>}
      {request.message && <p data-part="message">{request.message}</p>}
      <QuestionsCard request={request} disabled={disabled} onAnswer={onAnswer} />
    </>}
    {request.kind === "form" && <>
      <h3 id={titleId}>{request.server || "MCP form"}</h3>
      <p data-part="description">MCP server: {request.server}</p>
      {request.message && <p data-part="message">{request.message}</p>}
      <FormCard request={request} disabled={disabled} onAnswer={onAnswer} />
    </>}
    {request.kind === "tool-approval" && <>
      <h3 id={titleId}>Review tool request</h3>
      <p data-part="description">MCP server: {request.server}</p>
      {request.message && <p data-part="message">{request.message}</p>}
      <ToolApprovalCard request={request} titleId={titleId} disabled={disabled} onAnswer={onAnswer} />
    </>}
  </article>;
}

export function CodexInput({ bridge }: { bridge: UiBridge }) {
  const [view, setView] = useState<CodexView>();
  const [error, setError] = useState("");
  const [sending, setSending] = useState<string>();
  const sendingRef = useRef<string | undefined>(undefined);
  const mounted = useRef(false);
  const ownerRef = useRef<CodexOwner | undefined>(undefined);
  const container = useRef<HTMLElement | null>(null);
  const focusAfterRemoval = useRef<Element | undefined>(undefined);
  const [initializationError, setInitializationError] = useState("");

  useLayoutEffect(() => {
    mounted.current = true;
    setView(undefined);
    setError("");
    setInitializationError("");
    sendingRef.current = undefined;
    setSending(undefined);
    let owner: CodexOwner | undefined;
    let subscription: ReturnType<CodexOwner["subscribe"]> | undefined;
    try {
      owner = bridge.codex_input();
      ownerRef.current = owner;
      const subscriptionOwner = owner;
      subscription = owner.subscribe(snapshot => {
        const active = document.activeElement;
        const next = copySnapshot(snapshot);
        const card = active instanceof Element ? active.closest<HTMLElement>("[data-codex-card]") : null;
        const runId = card?.dataset.codexRun;
        const requestId = card?.dataset.codexRequest;
        if (active instanceof Element && runId && requestId &&
          !next.requests.some(request => request.run_id === runId && request.request_id === requestId)) focusAfterRemoval.current = active;
        if (mounted.current && ownerRef.current === subscriptionOwner) setView(next);
      });
    } catch (cause) {
      ownerRef.current = undefined;
      owner?.free();
      if (mounted.current) setInitializationError(cause instanceof Error ? cause.message : String(cause));
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
    const previous = focusAfterRemoval.current;
    if (!previous) return;
    if (previous.isConnected) {
      if (document.activeElement === document.body && previous instanceof HTMLButtonElement && !previous.disabled) {
        focusAfterRemoval.current = undefined;
        previous.focus();
      } else if (document.activeElement !== previous && document.activeElement !== document.body) focusAfterRemoval.current = undefined;
      return;
    }
    if (document.activeElement !== document.body) {
      focusAfterRemoval.current = undefined;
      return;
    }
    const next = container.current?.querySelector<HTMLButtonElement | HTMLInputElement | HTMLSelectElement>("[data-action='codex-input']:not(:disabled), input:not(:disabled), select:not(:disabled)");
    if (next) {
      focusAfterRemoval.current = undefined;
      next.focus();
    } else if (!view?.requests.length) {
      focusAfterRemoval.current = undefined;
      document.querySelector<HTMLTextAreaElement>("#bridge-question[data-prompt-field]")?.focus();
    }
  }, [view, sending]);

  function answer(request: CodexRequest, value: UiCodexAnswer, event: React.MouseEvent<HTMLButtonElement>) {
    const current = view;
    const owner = ownerRef.current;
    if (!mounted.current || !current || !owner || current.busy || current.stale || current.uncertain || !current.connected || !request.enabled || sendingRef.current) {
      value.free();
      return;
    }
    const key = requestKey(request.run_id, request.request_id);
    if (document.activeElement === event.currentTarget) focusAfterRemoval.current = event.currentTarget;
    sendingRef.current = key;
    setSending(key);
    setError("");
    try {
      void owner.send(request.run_id, request.request_id, value, event.nativeEvent).catch(cause => {
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

  const unavailable = Boolean(view && (!view.connected || view.stale || view.uncertain));
  const disabled = !view || unavailable || view.busy || Boolean(sending) || Boolean(initializationError);
  const hasContent = Boolean(initializationError || error || view?.notice || view?.requests.length || unavailable);

  return <section id="codex-input" ref={container} aria-label="Codex input requests" hidden={!hasContent}>
    <div id="codex-input-cards">
      {view?.requests.map(request => <CodexRequestCard key={requestKey(request.run_id, request.request_id)}
        request={request} disabled={disabled || !request.enabled || sending === requestKey(request.run_id, request.request_id)} onAnswer={answer} />)}
    </div>
    {unavailable ? <p role="alert">{view?.notice || (view?.uncertain ? "The answer state is uncertain." : view?.stale ? "The conversation changed." : "Connection lost. Reload to check Codex input requests.")}</p>
      : initializationError ? <p role="alert">{initializationError}</p>
      : error ? <p role="alert">{error}</p>
        : view?.notice && <p role="status">{view.notice}</p>}
    {(unavailable || initializationError) && <button type="button" data-action="codex-input-reload" onClick={() => window.location.reload()}>Reload</button>}
  </section>;
}
