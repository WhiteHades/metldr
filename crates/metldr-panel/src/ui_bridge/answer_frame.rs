use std::{
    cell::RefCell,
    num::NonZeroU32,
    rc::{Rc, Weak},
};

use js_sys::{Reflect, Uint8Array};
use metldr_answer_codec::{
    AnswerEvent, Binding, ChildMessage, HostMessage, decode_child, encode_host, encode_initialize,
    encode_program,
};
use metldr_conversation::{AnswerHeight, ArtifactCompletion, ArtifactId, GeneratedAnswer};
use metldr_history_pages::Counter;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use web_sys::{ReadableStreamDefaultReader, Request, RequestInit, RequestRedirect, Response};

use crate::model::{FlowError, RunId};

#[derive(Clone, Debug, Eq, PartialEq)]
struct Source {
    revision: u64,
    answer: GeneratedAnswer,
    completion: ArtifactCompletion,
}

#[derive(Default)]
enum Followup {
    #[default]
    Ready,
    Pending,
    Consumed,
}

#[derive(Default)]
enum Phase {
    #[default]
    AwaitingReady,
    Ready,
    Failed,
    Closed,
}

#[derive(Default)]
struct State {
    source: Option<Source>,
    waiting_floor: Option<Counter>,
    phase: Phase,
    pending: Option<u32>,
    next_id: u32,
    preview_sent: Option<(String, String)>,
    document_id: Option<NonZeroU32>,
    functions_sent: bool,
    expression_index: usize,
    followup: Followup,
}

impl State {
    fn should_read(&self, revision: Counter) -> Result<bool, String> {
        if matches!(self.phase, Phase::Closed) {
            return Err("Answer frame is closed.".into());
        }
        Ok(self.waiting_floor.is_none_or(|floor| revision.0 >= floor.0))
    }

    fn wait_for(&mut self, revision: Counter) {
        self.waiting_floor = Some(self.waiting_floor.map_or(revision, |floor| {
            if floor.0 >= revision.0 {
                floor
            } else {
                revision
            }
        }));
    }

    fn observe(
        &mut self,
        revision: u64,
        answer: GeneratedAnswer,
        completion: ArtifactCompletion,
    ) -> Result<(), String> {
        let next = Source {
            revision,
            answer,
            completion,
        };
        if let Some(current) = &self.source {
            if revision == current.revision && next == *current {
                self.waiting_floor = None;
                return Ok(());
            }
            if revision <= current.revision
                || current.completion != ArtifactCompletion::Streaming
                || !next.answer.extends(&current.answer)
            {
                return Err("answer source is not a valid continuation".into());
            }
        }
        self.source = Some(next);
        self.waiting_floor = None;
        Ok(())
    }

    fn ready(&mut self) -> Result<(), String> {
        if !matches!(self.phase, Phase::AwaitingReady) {
            return Err("answer frame is not awaiting ready".into());
        }
        self.phase = Phase::Ready;
        Ok(())
    }

    fn ack(&mut self, request_id: u32) -> Result<(), String> {
        if self.pending != Some(request_id) {
            return Err("answer frame acknowledgement is stale".into());
        }
        self.pending = None;
        Ok(())
    }

    fn command(&mut self) -> Result<Option<(u32, HostMessage)>, String> {
        if !matches!(self.phase, Phase::Ready) || self.pending.is_some() {
            return Ok(None);
        }
        let Some(source) = self.source.clone() else {
            return Ok(None);
        };
        let view = source.answer.view();
        if !view.css_complete {
            return Ok(None);
        }
        let css = view.css.unwrap_or("");
        let html = view.html.unwrap_or(&[]).concat();
        let interrupted = source.completion == ArtifactCompletion::Interrupted;
        let message = if matches!(self.followup, Followup::Consumed) {
            self.followup = Followup::Ready;
            HostMessage::FollowupConsumed {
                request_id: self.allocate()?,
            }
        } else if interrupted || !view.html_complete {
            if html.is_empty() {
                return Ok(None);
            }
            if self.preview_sent.as_ref() == Some(&(css.to_owned(), html.clone())) {
                return Ok(None);
            }
            self.preview_sent = Some((css.into(), html.clone()));
            self.document_id = None;
            HostMessage::Preview {
                request_id: self.allocate()?,
                css: css.into(),
                html,
            }
        } else if self.document_id.is_none() {
            if html.trim().is_empty() {
                return Ok(None);
            }
            let request_id = self.allocate()?;
            self.document_id = Some(request_id);
            HostMessage::Document {
                request_id,
                css: css.into(),
                html,
            }
        } else if !self.functions_sent && view.js_functions_complete {
            self.functions_sent = true;
            HostMessage::Functions {
                request_id: self.allocate()?,
                code: view.js_functions.unwrap_or("").into(),
            }
        } else if self.functions_sent {
            let expressions = view.js_expressions.unwrap_or(&[]);
            if self.expression_index < expressions.len() {
                let code = &expressions[self.expression_index];
                self.expression_index += 1;
                HostMessage::Expression {
                    request_id: self.allocate()?,
                    code: code.clone(),
                }
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        };
        let request_id = match &message {
            HostMessage::Preview { request_id, .. }
            | HostMessage::Document { request_id, .. }
            | HostMessage::Functions { request_id, .. }
            | HostMessage::Expression { request_id, .. }
            | HostMessage::FollowupConsumed { request_id } => request_id.get(),
        };
        self.pending = Some(request_id);
        Ok(Some((request_id, message)))
    }

    fn allocate(&mut self) -> Result<NonZeroU32, String> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|id| *id > 0)
            .ok_or("answer request id exhausted")?;
        NonZeroU32::new(self.next_id).ok_or_else(|| "answer request id exhausted".into())
    }

    fn propose_followup(&mut self) -> Result<(), String> {
        if !matches!(self.phase, Phase::Ready)
            || self.document_id.is_none()
            || !matches!(self.followup, Followup::Ready)
        {
            return Err("answer followup is not available".into());
        }
        self.followup = Followup::Pending;
        Ok(())
    }

    fn consume_followup(&mut self) -> Result<(), String> {
        if !matches!(self.phase, Phase::Ready) || !matches!(self.followup, Followup::Pending) {
            return Err("answer followup is not pending".into());
        }
        self.followup = Followup::Consumed;
        Ok(())
    }

    fn restart(&mut self) -> Result<(), String> {
        if matches!(self.phase, Phase::Closed) {
            return Err("closed answer frame cannot restart".into());
        }
        self.phase = Phase::AwaitingReady;
        self.pending = None;
        self.next_id = 0;
        self.preview_sent = None;
        self.document_id = None;
        self.functions_sent = false;
        self.expression_index = 0;
        self.followup = Followup::Ready;
        Ok(())
    }

    fn runtime_failed(&mut self, document_id: NonZeroU32) -> Result<(), String> {
        if !matches!(self.phase, Phase::Ready) || self.document_id != Some(document_id) {
            return Err("answer runtime failure is stale".into());
        }
        self.phase = Phase::Failed;
        self.pending = None;
        Ok(())
    }
}

#[wasm_bindgen]
pub struct UiAnswerFrame {
    core: Weak<super::Core>,
    binding: RefCell<Binding>,
    run: RunId,
    answer: ArtifactId,
    state: RefCell<State>,
}

impl UiAnswerFrame {
    pub(super) fn new(
        core: Rc<super::Core>,
        thread_id: String,
        run: String,
        answer: String,
    ) -> Result<Self, JsValue> {
        let run: RunId = serde_json::from_value(serde_json::Value::String(run))
            .map_err(|_| crate::err("Invalid answer run."))?;
        let answer = ArtifactId::new(answer).map_err(|_| crate::err("Invalid answer identity."))?;
        let store = &core.scope.store;
        validate_owned_id(run.as_str(), store)?;
        validate_owned_id(answer.as_str(), store)?;
        let binding = Binding::new(
            nonce()?,
            thread_id,
            run.as_str().into(),
            answer.as_str().into(),
        )
        .map_err(|error| crate::err(&error))?;
        Ok(Self {
            core: Rc::downgrade(&core),
            binding: RefCell::new(binding),
            run,
            answer,
            state: RefCell::new(State::default()),
        })
    }
}

#[wasm_bindgen]
impl UiAnswerFrame {
    pub fn initialize(&self) -> Result<String, JsValue> {
        self.current_core()?;
        encode_initialize(&self.binding.borrow()).map_err(|error| crate::err(&error))
    }

    pub async fn refresh(&self, revision: String) -> Result<bool, JsValue> {
        let revision: Counter = serde_json::from_value(serde_json::Value::String(revision))
            .map_err(|_| crate::err("Invalid answer revision."))?;
        if revision.0 == 0 {
            return Err(crate::err("Invalid answer revision."));
        }
        let core = self.current_core()?;
        {
            let state = self.state.borrow();
            if !state
                .should_read(revision)
                .map_err(|error| crate::err(&error))?
            {
                return Ok(false);
            }
        }
        let _read = core.begin_read().map_err(super::js_error)?;
        let flow = core.flow();
        let context = flow.history_current().await;
        self.current_core()?;
        if matches!(self.state.borrow().phase, Phase::Closed) {
            return Err(crate::err("Answer frame is closed."));
        }
        let context = context.map_err(super::js_error)?;
        if context.scope != core.scope {
            return Err(super::js_error(FlowError::HistoryStale));
        }
        let read = flow
            .history_generated(&context, &self.run, &self.answer, revision)
            .await;
        self.current_core()?;
        let mut state = self.state.borrow_mut();
        if matches!(state.phase, Phase::Closed) {
            return Err(crate::err("Answer frame is closed."));
        }
        match read.map_err(super::js_error)? {
            crate::history::GeneratedRead::Ready { answer, completion } => {
                state
                    .observe(revision.0, answer, completion)
                    .map_err(|error| crate::err(&error))?;
                Ok(true)
            }
            crate::history::GeneratedRead::Superseded(current) => {
                state.wait_for(current);
                Ok(false)
            }
        }
    }

    pub fn completion(&self) -> String {
        match self
            .state
            .borrow()
            .source
            .as_ref()
            .map(|source| source.completion)
        {
            None | Some(ArtifactCompletion::Streaming) => "streaming",
            Some(ArtifactCompletion::Completed) => "completed",
            Some(ArtifactCompletion::Interrupted) => "interrupted",
        }
        .into()
    }

    pub fn height(&self) -> u16 {
        self.state
            .borrow()
            .source
            .as_ref()
            .and_then(|source| source.answer.view().initial_height)
            .map_or(200, AnswerHeight::pixels)
    }

    pub fn has_content(&self) -> bool {
        self.state.borrow().source.as_ref().is_some_and(|source| {
            let view = source.answer.view();
            view.css_complete
                && view
                    .html
                    .is_some_and(|items| items.iter().any(|item| !item.trim().is_empty()))
        })
    }

    pub fn next_command(&self) -> Result<Option<String>, JsValue> {
        self.current_core()?;
        let mut state = self.state.borrow_mut();
        let Some((_, message)) = state.command().map_err(|error| crate::err(&error))? else {
            return Ok(None);
        };
        encode_host(&self.binding.borrow(), message)
            .map(Some)
            .map_err(|error| crate::err(&error))
    }

    pub fn receive(&self, encoded: String) -> Result<AnswerEvent, JsValue> {
        self.current_core()?;
        let received =
            decode_child(&encoded, &self.binding.borrow()).map_err(|error| crate::err(&error))?;
        let mut state = self.state.borrow_mut();
        if matches!(state.phase, Phase::Failed | Phase::Closed) {
            return Err(crate::err("Answer frame is closed."));
        }
        let message = match received {
            ChildMessage::Ready {} => {
                state.ready().map_err(|error| crate::err(&error))?;
                ChildMessage::Ready {}
            }
            ChildMessage::Ack { request_id } => {
                state
                    .ack(request_id.get())
                    .map_err(|error| crate::err(&error))?;
                ChildMessage::Ack { request_id }
            }
            ChildMessage::Height { height } => ChildMessage::Height {
                height: AnswerHeight::new(height)
                    .map_err(|error| crate::err(&error))?
                    .pixels() as f64,
            },
            ChildMessage::Followup { text } => {
                state
                    .propose_followup()
                    .map_err(|error| crate::err(&error))?;
                ChildMessage::Followup { text }
            }
            ChildMessage::Failed { request_id } => {
                if state.pending != Some(request_id.get()) {
                    return Err(crate::err("Answer frame request is stale."));
                }
                state.pending = None;
                ChildMessage::Failed { request_id }
            }
            ChildMessage::RuntimeFailed {
                document_id,
                reason,
            } => {
                state
                    .runtime_failed(document_id)
                    .map_err(|error| crate::err(&error))?;
                ChildMessage::RuntimeFailed {
                    document_id,
                    reason,
                }
            }
        };
        Ok(AnswerEvent::new(message))
    }

    pub fn restart(&self) -> Result<(), JsValue> {
        self.current_core()?;
        let frame_id = nonce()?;
        let mut state = self.state.borrow_mut();
        state.restart().map_err(|error| crate::err(&error))?;
        self.binding
            .borrow_mut()
            .set_frame_id(frame_id)
            .map_err(|error| crate::err(&error))?;
        Ok(())
    }

    pub fn consume_followup(&self) -> Result<(), JsValue> {
        self.current_core()?;
        self.state
            .borrow_mut()
            .consume_followup()
            .map_err(|error| crate::err(&error))
    }

    pub fn close(&self) {
        self.state.borrow_mut().phase = Phase::Closed;
    }

    pub async fn export_html(&self) -> Result<Option<String>, JsValue> {
        self.current_core()?;
        let (source, binding) = {
            let state = self.state.borrow();
            if matches!(state.phase, Phase::Closed) {
                return Err(crate::err("Answer frame is closed."));
            }
            let Some(source) = state
                .source
                .as_ref()
                .filter(|source| source.completion == ArtifactCompletion::Completed)
            else {
                return Ok(None);
            };
            (source.clone(), self.binding.borrow().clone())
        };
        let nonce = nonce()?;
        let runtime = answer_runtime().await?;
        self.current_core()?;
        {
            let state = self.state.borrow();
            if matches!(state.phase, Phase::Closed)
                || state.source.as_ref() != Some(&source)
                || *self.binding.borrow() != binding
            {
                return Err(crate::err(
                    "Answer changed while its export was being prepared.",
                ));
            }
        }
        source_html(&source.answer, &binding, &nonce, &runtime)
            .map(Some)
            .map_err(|error| crate::err(&error))
    }
}

impl UiAnswerFrame {
    fn current_core(&self) -> Result<Rc<super::Core>, JsValue> {
        let core = self
            .core
            .upgrade()
            .ok_or_else(|| crate::err("Answer frame owner is closed."))?;
        core.ensure_current().map_err(super::js_error)?;
        Ok(core)
    }
}

fn validate_owned_id(value: &str, store: &str) -> Result<(), JsValue> {
    let local = value
        .strip_prefix(&format!("{store}:"))
        .ok_or_else(|| crate::err("Invalid answer identity."))?;
    let number = local
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| crate::err("Invalid answer identity."))?;
    if local != number.to_string() {
        return Err(crate::err("Invalid answer identity."));
    }
    Ok(())
}

fn nonce() -> Result<String, JsValue> {
    let mut bytes = [0u8; 16];
    web_sys::window()
        .ok_or_else(|| crate::err("Secure randomness is unavailable."))?
        .crypto()?
        .get_random_values_with_u8_array(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn js_source_literal(code: &str) -> Result<String, String> {
    Ok(serde_json::to_string(code)
        .map_err(|error| error.to_string())?
        .replace('<', "\\u003c"))
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&#39;")
}

const ANSWER_RUNTIME_PATH: &str = "/chat/answer-frame.js";
const MAX_ANSWER_RUNTIME_BYTES: usize = 4 * 1024 * 1024;

async fn answer_runtime() -> Result<String, JsValue> {
    let window = web_sys::window().ok_or_else(|| crate::err("Answer export is unavailable."))?;
    let origin = window.location().origin()?;
    if !origin.starts_with("chrome-extension://") {
        return Err(crate::err("Answer export requires the extension page."));
    }
    let url = format!("{origin}{ANSWER_RUNTIME_PATH}");
    let init = RequestInit::new();
    init.set_redirect(RequestRedirect::Error);
    let request = Request::new_with_str_and_init(&url, &init)?;
    let response = JsFuture::from(window.fetch_with_request(&request)).await?;
    let response: Response = response.dyn_into()?;
    if !response.ok() || response.url() != url {
        return Err(crate::err("Packaged answer runtime could not be loaded."));
    }
    if response
        .headers()
        .get("content-length")?
        .and_then(|length| length.parse::<usize>().ok())
        .is_some_and(|length| length > MAX_ANSWER_RUNTIME_BYTES)
    {
        return Err(crate::err(
            "Packaged answer runtime exceeds its size limit.",
        ));
    }
    let body = response
        .body()
        .ok_or_else(|| crate::err("Packaged answer runtime has no body."))?;
    let reader: ReadableStreamDefaultReader = body.get_reader().dyn_into()?;
    let mut bytes = Vec::new();
    loop {
        let result = JsFuture::from(reader.read()).await?;
        if Reflect::get(&result, &"done".into())?.as_bool() == Some(true) {
            break;
        }
        let chunk = Uint8Array::new(&Reflect::get(&result, &"value".into())?);
        let length = chunk.length() as usize;
        if bytes.len().saturating_add(length) > MAX_ANSWER_RUNTIME_BYTES {
            let _ = reader.cancel();
            return Err(crate::err(
                "Packaged answer runtime exceeds its size limit.",
            ));
        }
        bytes.extend_from_slice(&chunk.to_vec());
    }
    String::from_utf8(bytes).map_err(|_| crate::err("Packaged answer runtime is not valid UTF-8."))
}

fn source_html(
    source: &GeneratedAnswer,
    binding: &Binding,
    nonce: &str,
    runtime: &str,
) -> Result<String, String> {
    let program = encode_program(binding, source)?.replace('<', "\\u003c");
    let runtime = js_source_literal(runtime)?;
    let base_css = js_source_literal(include_str!("../../assets/answer-frame.css"))?;
    let nonce_literal = js_source_literal(nonce)?;
    let inner = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data: blob:; style-src 'unsafe-inline'; script-src 'nonce-{nonce}' 'unsafe-eval' blob:; worker-src blob:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'; media-src 'none'; font-src data:\"></head><body><script type=\"application/json\" id=\"metldr-answer-program\">{program}</script><script type=\"module\" nonce=\"{nonce}\">const style=document.createElement('style');style.textContent={base_css};document.head.append(style);const script=document.createElement('script');script.type='module';script.nonce={nonce_literal};script.textContent={runtime};document.head.append(script);</script></body></html>"
    );
    let inner = escape_html_attribute(&inner);
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'nonce-{nonce}' 'unsafe-eval' blob:; img-src data: blob:; style-src 'unsafe-inline'; frame-src about:; worker-src blob:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'\"><style>body{{margin:0}}iframe{{display:block;inline-size:100%;block-size:100dvh;border:0}}</style><title>metldr answer</title></head><body><iframe title=\"Interactive answer\" sandbox=\"allow-scripts\" srcdoc=\"{inner}\"></iframe></body></html>"
    ))
}

#[cfg(test)]
mod checks {
    use super::*;
    use metldr_conversation::GeneratedUpdate;

    fn update(source: &mut GeneratedAnswer, update: GeneratedUpdate) {
        *source = source.updated(update).unwrap();
    }

    fn partial(html: &str) -> GeneratedAnswer {
        let mut source = GeneratedAnswer::default();
        update(&mut source, GeneratedUpdate::Css("p{}".into()));
        update(&mut source, GeneratedUpdate::Html(html.into()));
        update(&mut source, GeneratedUpdate::Functions("work()".into()));
        update(
            &mut source,
            GeneratedUpdate::Expression("sendPrompt('x')".into()),
        );
        source
    }

    fn finish(source: &mut GeneratedAnswer) {
        update(source, GeneratedUpdate::CssComplete);
        update(source, GeneratedUpdate::HtmlComplete);
        update(source, GeneratedUpdate::FunctionsComplete);
        update(source, GeneratedUpdate::ExpressionsComplete);
    }

    #[test]
    fn runtime_failure_is_terminal_after_ack_until_restart() {
        let mut state = State::default();
        let mut source = partial("<p>ready</p>");
        finish(&mut source);
        state
            .observe(1, source, ArtifactCompletion::Completed)
            .unwrap();
        state.ready().unwrap();
        let (_, HostMessage::Document { request_id, .. }) = state.command().unwrap().unwrap()
        else {
            panic!()
        };
        state.ack(request_id.get()).unwrap();
        assert!(
            state
                .runtime_failed(NonZeroU32::new(request_id.get() + 1).unwrap())
                .is_err()
        );
        state.runtime_failed(request_id).unwrap();
        assert!(state.command().unwrap().is_none());
        assert!(state.ready().is_err());
        assert!(state.propose_followup().is_err());
        state.restart().unwrap();
        state.ready().unwrap();
        assert!(matches!(
            state.command().unwrap(),
            Some((_, HostMessage::Document { .. }))
        ));
    }

    #[test]
    fn command_order_serializes_updates_and_enforces_one_ack() {
        let mut waiting = State::default();
        waiting
            .observe(1, partial("<p>waiting"), ArtifactCompletion::Streaming)
            .unwrap();
        waiting.ready().unwrap();
        assert!(waiting.command().unwrap().is_none());
        let mut state = State::default();
        let mut initial = partial("<p>a");
        update(&mut initial, GeneratedUpdate::CssComplete);
        state
            .observe(1, initial.clone(), ArtifactCompletion::Streaming)
            .unwrap();
        state.ready().unwrap();
        let (_, HostMessage::Preview { html, .. }) = state.command().unwrap().unwrap() else {
            panic!()
        };
        assert_eq!(html, "<p>a");
        assert!(state.command().unwrap().is_none());
        update(&mut initial, GeneratedUpdate::Html("b".into()));
        state
            .observe(2, initial.clone(), ArtifactCompletion::Streaming)
            .unwrap();
        assert!(state.ack(1).is_ok());
        let (preview_id, HostMessage::Preview { html, .. }) = state.command().unwrap().unwrap()
        else {
            panic!()
        };
        assert_eq!(html, "<p>ab");
        assert!(state.ack(1).is_err());
        state.ack(preview_id).unwrap();

        update(&mut initial, GeneratedUpdate::CssComplete);
        update(&mut initial, GeneratedUpdate::HtmlComplete);
        state
            .observe(3, initial.clone(), ArtifactCompletion::Streaming)
            .unwrap();
        assert!(matches!(
            state.command().unwrap(),
            Some((_, HostMessage::Document { .. }))
        ));
        assert!(state.command().unwrap().is_none());
        let document_id = state.pending.unwrap();
        state.ack(document_id).unwrap();
        assert!(state.command().unwrap().is_none());

        update(&mut initial, GeneratedUpdate::FunctionsComplete);
        state
            .observe(4, initial.clone(), ArtifactCompletion::Streaming)
            .unwrap();
        assert!(matches!(
            state.command().unwrap(),
            Some((_, HostMessage::Functions { .. }))
        ));
        let functions_id = state.pending.unwrap();
        state.ack(functions_id).unwrap();
        assert!(matches!(
            state.command().unwrap(),
            Some((_, HostMessage::Expression { .. }))
        ));
        assert!(state.command().unwrap().is_none());
        let expression_id = state.pending.unwrap();
        state.ack(expression_id).unwrap();
        update(&mut initial, GeneratedUpdate::Expression("later()".into()));
        state
            .observe(5, initial.clone(), ArtifactCompletion::Streaming)
            .unwrap();
        assert!(matches!(
            state.command().unwrap(),
            Some((_, HostMessage::Expression { code, .. })) if code == "later()"
        ));
        let next_expression_id = state.pending.unwrap();
        state.ack(next_expression_id).unwrap();
        update(&mut initial, GeneratedUpdate::ExpressionsComplete);
        state
            .observe(6, initial, ArtifactCompletion::Completed)
            .unwrap();
        assert!(state.command().unwrap().is_none());
        state.propose_followup().unwrap();
        assert!(state.propose_followup().is_err());
        state.consume_followup().unwrap();
        assert!(state.propose_followup().is_err());
        let (consumed, HostMessage::FollowupConsumed { .. }) = state.command().unwrap().unwrap()
        else {
            panic!()
        };
        state.ack(consumed).unwrap();
        state.propose_followup().unwrap();
        assert!(
            state
                .observe(7, partial("<p>changed"), ArtifactCompletion::Streaming)
                .is_err()
        );
    }

    #[test]
    fn interrupted_output_stays_passive_and_export_neutralizes_script_terminators() {
        let binding = Binding::new("0".repeat(32), "t".into(), "r".into(), "a".into()).unwrap();
        let nonce = "1".repeat(32);
        let mut source = partial("<p>safe");
        finish(&mut source);
        let mut state = State::default();
        state
            .observe(1, source.clone(), ArtifactCompletion::Interrupted)
            .unwrap();
        state.ready().unwrap();
        assert!(matches!(
            state.command().unwrap(),
            Some((_, HostMessage::Preview { .. }))
        ));
        let request = state.pending.unwrap();
        state.ack(request).unwrap();
        assert!(state.command().unwrap().is_none());
        assert!(
            source_html(&source, &binding, &nonce, "runtime")
                .unwrap()
                .contains("default-src 'none'")
        );

        let mut hostile = GeneratedAnswer::default();
        let hostile_html = "<p>hi</p><script>sendPrompt('early')</script>";
        let hostile_css = "p{color:red}</style><script>sendPrompt('css')</script>";
        update(&mut hostile, GeneratedUpdate::Html(hostile_html.into()));
        update(&mut hostile, GeneratedUpdate::Css(hostile_css.into()));
        update(&mut hostile, GeneratedUpdate::HtmlComplete);
        update(&mut hostile, GeneratedUpdate::CssComplete);
        let code = "if (1 < 2) sendPrompt('ok'); /* </script> */";
        update(&mut hostile, GeneratedUpdate::Functions(code.into()));
        update(&mut hostile, GeneratedUpdate::FunctionsComplete);
        update(
            &mut hostile,
            GeneratedUpdate::Expression("Promise.resolve('<ok>')".into()),
        );
        update(&mut hostile, GeneratedUpdate::ExpressionsComplete);
        let literal = js_source_literal(code).unwrap();
        assert_eq!(serde_json::from_str::<String>(&literal).unwrap(), code);
        assert!(!literal.contains('<'));
        for value in [hostile_html, hostile_css] {
            let literal = js_source_literal(value).unwrap();
            assert_eq!(serde_json::from_str::<String>(&literal).unwrap(), value);
            assert!(!literal.contains('<'));
        }
        let output = source_html(&hostile, &binding, &nonce, "runtime < /script").unwrap();
        assert!(output.contains("\\u003c/script&gt;"));
        assert!(output.contains("sandbox=\"allow-scripts\""));
        assert!(output.contains("frame-src about:"));
        assert!(output.contains("script-src &#39;nonce-11111111111111111111111111111111&#39; &#39;unsafe-eval&#39; blob:"));
        assert!(!output.contains("script-src &#39;unsafe-inline&#39;"));
        assert!(output.contains("connect-src &#39;none&#39;"));
        assert!(output.contains("application/json"));
        assert!(output.contains("metldr-answer-program"));
        assert!(output.contains("runtime \\u003c /script"));
        let wrong = Binding::new("0".repeat(32), "other".into(), "r".into(), "a".into()).unwrap();
        let ready = r#"{"binding":{"v":1,"frameId":"00000000000000000000000000000000","threadId":"t","runId":"r","answerId":"a"},"message":{"kind":"ready"}}"#;
        assert!(decode_child(ready, &binding).is_ok());
        assert!(decode_child(ready, &wrong).is_err());
        let encoded = r#"{"binding":{"v":1,"frameId":"00000000000000000000000000000000","threadId":"t","runId":"r","answerId":"a"},"message":{"kind":"ready","extra":true}}"#;
        assert!(decode_child(encoded, &binding).is_err());
        assert_eq!(
            serde_json::to_string(&ChildMessage::Ready {}).unwrap(),
            r#"{"kind":"ready"}"#
        );
    }

    #[test]
    fn waiting_floor_tracks_supersession_until_terminal_observation() {
        let mut state = State::default();
        state.wait_for(Counter(3));
        state.wait_for(Counter(5));
        state.wait_for(Counter(4));
        assert!(!state.should_read(Counter(4)).unwrap());
        assert!(state.should_read(Counter(5)).unwrap());

        let mut terminal = partial("<p>done</p>");
        finish(&mut terminal);
        state
            .observe(5, terminal, ArtifactCompletion::Completed)
            .unwrap();
        assert_eq!(
            state.source.as_ref().unwrap().completion,
            ArtifactCompletion::Completed
        );
        assert!(state.waiting_floor.is_none());
        assert!(state.should_read(Counter(4)).unwrap());

        state.phase = Phase::Closed;
        assert!(state.should_read(Counter(6)).is_err());
    }
}
