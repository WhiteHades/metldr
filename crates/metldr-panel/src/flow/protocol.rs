use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
};

use serde_json::Value;

use crate::model::{FlowError, PageView, RunId};

const MAX_PENDING: usize = 16;
const MAX_WATCHES: usize = 16;
const MAX_OBSERVERS: usize = 16;
const MAX_REQUEST_ID: usize = 96;
const MAX_ERROR: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReplyKind {
    History(crate::history::Expected),
    Page,
    SelectedPage(metldr_history_pages::Scope),
    Start(metldr_history_pages::Scope),
    Stop(metldr_history_pages::Scope, RunId),
    Correction(metldr_history_pages::Scope, RunId, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    History(crate::history::Response),
    Page(PageView),
    Started(RunId),
    Stop(StopReceipt),
    Correction(crate::model::CorrectionReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StopReceipt {
    Requested,
    AlreadyTerminal,
}

pub(crate) struct ReplyCell {
    result: Option<Result<Reply, FlowError>>,
    waker: Option<Waker>,
}

impl ReplyCell {
    fn new() -> Self {
        Self {
            result: None,
            waker: None,
        }
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<Reply, FlowError>> {
        if let Some(result) = self.result.take() {
            Poll::Ready(result)
        } else {
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    pub(crate) fn fail(&mut self, error: FlowError) -> Option<Waker> {
        self.resolve(Err(error))
    }

    fn resolve(&mut self, result: Result<Reply, FlowError>) -> Option<Waker> {
        if self.result.is_some() {
            return None;
        }
        self.result = Some(result);
        self.waker.take()
    }
}

pub(crate) struct WatchCell {
    target: Option<(metldr_history_pages::Scope, RunId)>,
    queue: Option<PageView>,
    terminal_queued: bool,
    last_observed: Option<(String, u64)>,
    error: Option<FlowError>,
    closed: bool,
    close_after_error: bool,
    waker: Option<Waker>,
}

impl WatchCell {
    pub(crate) fn terminal_received(&self) -> bool {
        self.terminal_queued
    }

    fn specific(scope: metldr_history_pages::Scope, run: RunId) -> Self {
        Self {
            target: Some((scope, run)),
            queue: None,
            terminal_queued: false,
            last_observed: None,
            error: None,
            closed: false,
            close_after_error: false,
            waker: None,
        }
    }

    fn observer(latest: Option<&PageView>) -> Self {
        let mut cell = Self {
            target: None,
            queue: None,
            terminal_queued: false,
            last_observed: None,
            error: None,
            closed: false,
            close_after_error: false,
            waker: None,
        };
        if let Some(view) = latest {
            cell.queue = Some(view.clone());
            cell.last_observed = Some((view.store.clone(), view.sequence));
        }
        cell
    }

    pub(crate) fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<PageView>, FlowError>> {
        if let Some(error) = self.error.take() {
            if self.close_after_error {
                self.closed = true;
            }
            return Poll::Ready(Err(error));
        }
        if let Some(view) = self.queue.take() {
            if self.target.is_some() && self.terminal_queued {
                self.closed = true;
            }
            return Poll::Ready(Ok(Some(view)));
        }
        if self.closed {
            return Poll::Ready(Ok(None));
        }
        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn wake(&mut self) -> Option<Waker> {
        self.waker.take()
    }

    fn observe(&mut self, view: &PageView) -> Option<Waker> {
        if self.closed || self.target.is_some() {
            return None;
        }
        let key = (view.store.clone(), view.sequence);
        if self.last_observed.as_ref() == Some(&key) {
            return None;
        }
        self.last_observed = Some(key);
        self.queue = Some(view.clone());
        self.wake()
    }

    fn deliver(&mut self, view: PageView, ended: bool) -> Result<Option<Waker>, FlowError> {
        if self.closed || self.terminal_queued {
            return Ok(None);
        }
        let (scope, run) = self
            .target
            .as_ref()
            .ok_or_else(|| FlowError::Protocol("view response was routed to an observer".into()))?;
        if scope != &view.selection {
            return Err(FlowError::Protocol(
                "watch selection does not match request".into(),
            ));
        }
        let found = view
            .runs
            .iter()
            .find(|saved| &saved.id == run)
            .ok_or_else(|| FlowError::Protocol("watch snapshot does not include its run".into()))?;
        if found.status.is_terminal() != ended {
            return Err(FlowError::Protocol(
                "watch terminal flag does not match run status".into(),
            ));
        }
        let key = (view.store.clone(), view.sequence);
        if self.last_observed.as_ref() == Some(&key) {
            return Ok(None);
        }
        if self
            .last_observed
            .as_ref()
            .is_some_and(|(store, sequence)| store == &view.store && *sequence > view.sequence)
        {
            return Ok(None);
        }
        self.last_observed = Some(key);
        if self
            .queue
            .as_ref()
            .is_some_and(|queued| queued.store == view.store && queued.sequence >= view.sequence)
        {
            return Ok(None);
        }
        self.queue = Some(view);
        self.terminal_queued = ended;
        Ok(self.wake())
    }

    fn fail(&mut self, error: FlowError, close_after: bool) -> Option<Waker> {
        if self.closed || self.terminal_queued {
            return None;
        }
        if self.error.is_some() {
            self.close_after_error |= close_after;
            return self.wake();
        }
        self.queue = None;
        self.error = Some(error);
        self.close_after_error = close_after;
        self.wake()
    }

    fn close(&mut self) {
        self.closed = true;
        self.queue = None;
        self.error = None;
        self.waker.take();
    }
}

pub(crate) struct Protocol {
    pub(crate) latest: Option<PageView>,
    pub(crate) connected: bool,
    pub(crate) closed: bool,
    pending: BTreeMap<String, (ReplyKind, Weak<RefCell<ReplyCell>>)>,
    watches: BTreeMap<String, (metldr_history_pages::Scope, RunId, Weak<RefCell<WatchCell>>)>,
    observers: Vec<Weak<RefCell<WatchCell>>>,
}

impl Default for Protocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Protocol {
    pub(crate) fn new() -> Self {
        Self {
            latest: None,
            connected: false,
            closed: false,
            pending: BTreeMap::new(),
            watches: BTreeMap::new(),
            observers: Vec::new(),
        }
    }

    pub(crate) fn register_request(
        &mut self,
        id: String,
        kind: ReplyKind,
    ) -> Result<Rc<RefCell<ReplyCell>>, FlowError> {
        self.prune();
        self.validate_registration(&id)?;
        if self.pending.len() >= MAX_PENDING || self.pending.contains_key(&id) {
            return Err(FlowError::Protocol(
                "pending request capacity or ID conflict".into(),
            ));
        }
        let cell = Rc::new(RefCell::new(ReplyCell::new()));
        self.pending.insert(id, (kind, Rc::downgrade(&cell)));
        Ok(cell)
    }

    pub(crate) fn remove_request(&mut self, id: &str) {
        self.pending.remove(id);
    }

    pub(crate) fn register_watch(
        &mut self,
        id: String,
        scope: metldr_history_pages::Scope,
        run: RunId,
    ) -> Result<Rc<RefCell<WatchCell>>, FlowError> {
        self.prune();
        self.validate_registration(&id)?;
        if self.watches.len() >= MAX_WATCHES || self.watches.contains_key(&id) {
            return Err(FlowError::Protocol("watch capacity or ID conflict".into()));
        }
        let cell = Rc::new(RefCell::new(WatchCell::specific(
            scope.clone(),
            run.clone(),
        )));
        self.watches.insert(id, (scope, run, Rc::downgrade(&cell)));
        Ok(cell)
    }

    pub(crate) fn remove_watch(&mut self, id: &str) {
        if let Some((_, _, cell)) = self.watches.remove(id) {
            if let Some(cell) = cell.upgrade() {
                cell.borrow_mut().close();
            }
        }
    }

    pub(crate) fn observe(&mut self) -> Result<Rc<RefCell<WatchCell>>, FlowError> {
        self.prune();
        if self.closed {
            return Err(FlowError::Disconnected);
        }
        if self.observers.len() >= MAX_OBSERVERS {
            return Err(FlowError::Protocol("observer capacity reached".into()));
        }
        let cell = Rc::new(RefCell::new(WatchCell::observer(self.latest.as_ref())));
        self.observers.push(Rc::downgrade(&cell));
        Ok(cell)
    }

    pub(crate) fn remove_observer(&mut self, observer: &Rc<RefCell<WatchCell>>) {
        self.observers.retain(|weak| {
            weak.upgrade()
                .is_some_and(|cell| !Rc::ptr_eq(&cell, observer))
        });
        observer.borrow_mut().close();
    }

    pub(crate) fn receive(&mut self, value: Value) -> Result<Vec<Waker>, FlowError> {
        if self.closed {
            return Ok(Vec::new());
        }
        let object = value
            .as_object()
            .ok_or_else(|| FlowError::Protocol("host frame must be an object".into()))?;
        let version = value.get("v").and_then(Value::as_u64);
        let request = value.get("request");
        let kind = value.get("kind").and_then(Value::as_str);
        if version != Some(u64::from(crate::WIRE_VERSION)) || request.is_none() || kind.is_none() {
            return Err(FlowError::Protocol("invalid host frame envelope".into()));
        }
        let kind = kind.unwrap();
        let request = request.unwrap();
        if kind == "disconnected" {
            if !exact_fields(object, &["v", "request", "kind"]) || !request.is_null() {
                return Err(FlowError::Protocol("invalid disconnected frame".into()));
            }
            self.connected = false;
            return Ok(self.disconnect(FlowError::Disconnected, false));
        }

        if request.is_null() {
            if !matches!(kind, "ready" | "changed") {
                return Err(FlowError::Protocol(
                    "unexpected unsolicited host frame".into(),
                ));
            }
            let view = decode_snapshot(&value, object, kind)?;
            self.validate_view(&view, kind == "ready")?;
            self.connected = true;
            return Ok(self.update_view(view, kind == "ready"));
        }
        let request_id = request.as_str().ok_or_else(|| {
            FlowError::Protocol("host request ID must be a string or null".into())
        })?;
        if !valid_request_id(request_id) {
            return Err(FlowError::Protocol("invalid host request ID".into()));
        }

        if kind == "view" {
            if let Some((expected_scope, expected_run, cell)) =
                self.watches.get(request_id).cloned()
            {
                if !exact_fields(object, &["v", "request", "kind", "view", "ended"]) {
                    return self.fail_watch(
                        request_id,
                        FlowError::Protocol("invalid watch frame fields".into()),
                    );
                }
                let view = match decode_view(&value) {
                    Ok(view) => view,
                    Err(error) => return self.fail_watch(request_id, error),
                };
                if let Err(error) = self.validate_view(&view, false) {
                    return self.fail_watch(request_id, error);
                }
                let ended = match value.get("ended").and_then(Value::as_bool) {
                    Some(ended) => ended,
                    None => {
                        return self.fail_watch(
                            request_id,
                            FlowError::Protocol("watch frame missing terminal flag".into()),
                        );
                    }
                };
                let Some(cell) = cell.upgrade() else {
                    self.watches.remove(request_id);
                    return Ok(Vec::new());
                };
                if view.selection != expected_scope
                    || !view.runs.iter().any(|saved| saved.id == expected_run)
                {
                    return self.fail_watch(
                        request_id,
                        FlowError::Protocol("watch snapshot does not match scope and run".into()),
                    );
                }
                let delivery = { cell.borrow_mut().deliver(view.clone(), ended) };
                let mut wakers: Vec<Waker> = match delivery {
                    Ok(waker) => waker.into_iter().collect(),
                    Err(error) => return self.fail_watch(request_id, error),
                };
                wakers.extend(self.update_view(view, false));
                self.connected = true;
                if ended {
                    self.watches.remove(request_id);
                }
                return Ok(wakers);
            }
            let Some((expected, weak)) = self.pending.get(request_id).cloned() else {
                return Ok(Vec::new());
            };
            if !matches!(expected, ReplyKind::Page | ReplyKind::SelectedPage(_)) {
                return self.fail_request(
                    request_id,
                    FlowError::Protocol("reply kind does not match request".into()),
                );
            }
            if !exact_fields(object, &["v", "request", "kind", "view", "ended"])
                || value.get("ended").and_then(Value::as_bool) != Some(true)
            {
                return self
                    .fail_request(request_id, FlowError::Protocol("invalid page reply".into()));
            }
            let view = match decode_view(&value) {
                Ok(view) => view,
                Err(error) => return self.fail_request(request_id, error),
            };
            if matches!(&expected, ReplyKind::SelectedPage(scope) if *scope != view.selection) {
                return self.fail_request(
                    request_id,
                    FlowError::Protocol("capture selection changed".into()),
                );
            }
            if let Err(error) = self.validate_view(&view, false) {
                return self.fail_request(request_id, error);
            }
            self.pending.remove(request_id);
            self.connected = true;
            let mut wakers = weak
                .upgrade()
                .and_then(|cell| cell.borrow_mut().resolve(Ok(Reply::Page(view.clone()))))
                .into_iter()
                .collect::<Vec<_>>();
            wakers.extend(self.update_view(view, false));
            return Ok(wakers);
        }

        if kind == "error" {
            let Some((_, _)) = self.pending.get(request_id) else {
                if !self.watches.contains_key(request_id) {
                    return Ok(Vec::new());
                }
                let message = match parse_error(&value, object) {
                    Ok(message) => message,
                    Err(error) => return self.fail_watch(request_id, error),
                };
                return self.fail_watch(request_id, FlowError::Host(message));
            };
            let message = match parse_error(&value, object) {
                Ok(message) => message,
                Err(error) => return self.fail_request(request_id, error),
            };
            let (_, cell) = self.pending.remove(request_id).unwrap();
            return Ok(cell
                .upgrade()
                .and_then(|cell| cell.borrow_mut().fail(FlowError::Remote(message)))
                .into_iter()
                .collect());
        }

        let Some((expected, weak)) = self.pending.get(request_id).cloned() else {
            return Ok(Vec::new());
        };
        if let ReplyKind::History(expected) = expected {
            let response = match expected.decode(value.clone()) {
                Ok(response) => response,
                Err(error) => return self.fail_request(request_id, error),
            };
            self.pending.remove(request_id);
            return Ok(weak
                .upgrade()
                .and_then(|cell| cell.borrow_mut().resolve(Ok(Reply::History(response))))
                .into_iter()
                .collect());
        }
        let (reply, fields) = match (expected, kind) {
            (ReplyKind::Start(expected_scope), "started") => match parse_run(&value) {
                Ok(run) if parse_scope(&value).as_ref() == Ok(&expected_scope) => (
                    Reply::Started(run),
                    &["v", "request", "kind", "scope", "run"][..],
                ),
                Ok(_) => {
                    return self.fail_request(
                        request_id,
                        FlowError::Protocol("start receipt scope does not match request".into()),
                    );
                }
                Err(error) => return self.fail_request(request_id, error),
            },
            (ReplyKind::Stop(expected_scope, expected_run), "stopping") => {
                match (parse_run(&value), parse_scope(&value)) {
                    (Ok(run), Ok(scope)) if run == expected_run && scope == expected_scope => (
                        Reply::Stop(StopReceipt::Requested),
                        &["v", "request", "kind", "scope", "run"][..],
                    ),
                    (Ok(_), Ok(_)) => {
                        return self.fail_request(
                            request_id,
                            FlowError::Protocol(
                                "stop receipt scope or run does not match request".into(),
                            ),
                        );
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        return self.fail_request(request_id, error);
                    }
                }
            }
            (ReplyKind::Stop(expected_scope, expected_run), "already_terminal") => {
                match (parse_run(&value), parse_scope(&value)) {
                    (Ok(run), Ok(scope)) if run == expected_run && scope == expected_scope => (
                        Reply::Stop(StopReceipt::AlreadyTerminal),
                        &["v", "request", "kind", "scope", "run"][..],
                    ),
                    (Ok(_), Ok(_)) => {
                        return self.fail_request(
                            request_id,
                            FlowError::Protocol(
                                "stop receipt scope or run does not match request".into(),
                            ),
                        );
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        return self.fail_request(request_id, error);
                    }
                }
            }
            (ReplyKind::Correction(expected_scope, expected_run, expected_text), "correction") => {
                match (parse_run(&value), parse_scope(&value)) {
                    (Ok(run), Ok(scope)) if run == expected_run && scope == expected_scope => {
                        let receipt: Result<crate::model::CorrectionReceipt, FlowError> = value
                            .get("receipt")
                            .cloned()
                            .ok_or_else(|| {
                                FlowError::Protocol("correction receipt is missing".into())
                            })
                            .and_then(|value| {
                                serde_json::from_value(value)
                                    .map_err(|error| FlowError::Protocol(error.to_string()))
                            });
                        let receipt = match receipt {
                            Ok(receipt) => receipt,
                            Err(error) => return self.fail_request(request_id, error),
                        };
                        if !(1..=16).contains(&receipt.ordinal)
                            || receipt.request != request_id
                            || receipt.text.text != expected_text
                            || receipt.text.truncated
                            || !crate::model::valid_correction_receipt(&receipt, 1024, true)
                        {
                            return self.fail_request(
                                request_id,
                                FlowError::Protocol(
                                    "correction receipt does not match the captured submission"
                                        .into(),
                                ),
                            );
                        }
                        (
                            Reply::Correction(receipt),
                            &["v", "request", "kind", "scope", "run", "receipt"][..],
                        )
                    }
                    (Ok(_), Ok(_)) => {
                        return self.fail_request(
                            request_id,
                            FlowError::Protocol(
                                "correction receipt scope or run does not match request".into(),
                            ),
                        );
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        return self.fail_request(request_id, error);
                    }
                }
            }
            _ => {
                return self.fail_request(
                    request_id,
                    FlowError::Protocol("reply kind does not match request".into()),
                );
            }
        };
        if !exact_fields(object, fields) {
            return self.fail_request(
                request_id,
                FlowError::Protocol("invalid reply fields".into()),
            );
        }
        self.pending.remove(request_id);
        self.connected = true;
        Ok(weak
            .upgrade()
            .and_then(|cell| cell.borrow_mut().resolve(Ok(reply)))
            .into_iter()
            .collect())
    }

    pub(crate) fn disconnect(&mut self, error: FlowError, permanent: bool) -> Vec<Waker> {
        self.connected = false;
        if permanent {
            self.closed = true;
        }
        let mut wakers = Vec::new();
        for (_, weak) in std::mem::take(&mut self.pending).into_values() {
            if let Some(waker) = weak
                .upgrade()
                .and_then(|cell| cell.borrow_mut().fail(error.clone()))
            {
                wakers.push(waker);
            }
        }
        let watches = std::mem::take(&mut self.watches);
        for (_, _, weak) in watches.into_values() {
            if let Some(cell) = weak.upgrade() {
                let mut cell = cell.borrow_mut();
                if let Some(waker) = cell.fail(error.clone(), true) {
                    wakers.push(waker);
                }
            }
        }
        self.observers.retain(|weak| {
            let Some(observer) = weak.upgrade() else {
                return false;
            };
            let mut observer = observer.borrow_mut();
            observer.last_observed = None;
            if let Some(waker) = observer.fail(error.clone(), permanent) {
                wakers.push(waker);
            }
            !observer.closed
        });
        wakers
    }

    fn validate_view(&self, view: &PageView, ready: bool) -> Result<(), FlowError> {
        if let Some(previous) = &self.latest {
            if previous.store != view.store {
                if !ready {
                    return Err(FlowError::Protocol(
                        "store changed without readiness".into(),
                    ));
                }
            } else if (previous.selection.epoch == view.selection.epoch
                && previous.selection != view.selection)
                || (previous.selection.epoch.0 < view.selection.epoch.0
                    && previous.sequence >= view.sequence)
                || (previous.selection.epoch.0 > view.selection.epoch.0
                    && previous.sequence <= view.sequence)
            {
                return Err(FlowError::Protocol(
                    "view selection and sequence differ".into(),
                ));
            }
        }
        Ok(())
    }

    fn update_view(&mut self, view: PageView, reconnect: bool) -> Vec<Waker> {
        let accept = self.latest.as_ref().is_none_or(|latest| {
            latest.store != view.store
                || (latest.sequence < view.sequence && !view.is_older_than(latest))
        });
        if accept {
            self.latest = Some(view.clone());
        }
        if !accept && !reconnect {
            return Vec::new();
        }
        let Some(latest) = self.latest.as_ref() else {
            return Vec::new();
        };
        self.observers
            .iter()
            .filter_map(Weak::upgrade)
            .filter_map(|observer| observer.borrow_mut().observe(latest))
            .collect()
    }

    fn fail_request(&mut self, id: &str, error: FlowError) -> Result<Vec<Waker>, FlowError> {
        let (_, weak) = self.pending.remove(id).expect("registered request");
        Ok(weak
            .upgrade()
            .and_then(|cell| cell.borrow_mut().fail(error))
            .into_iter()
            .collect())
    }

    fn fail_watch(&mut self, id: &str, error: FlowError) -> Result<Vec<Waker>, FlowError> {
        let Some((_, _, weak)) = self.watches.remove(id) else {
            return Ok(Vec::new());
        };
        Ok(weak
            .upgrade()
            .and_then(|cell| cell.borrow_mut().fail(error, true))
            .into_iter()
            .collect())
    }

    fn validate_registration(&self, id: &str) -> Result<(), FlowError> {
        if self.closed
            || !valid_request_id(id)
            || self.pending.contains_key(id)
            || self.watches.contains_key(id)
        {
            return Err(FlowError::Protocol(
                "invalid or duplicate request ID".into(),
            ));
        }
        Ok(())
    }

    fn prune(&mut self) {
        self.pending.retain(|_, (_, weak)| weak.strong_count() > 0);
        self.watches
            .retain(|_, (_, _, weak)| weak.strong_count() > 0);
        self.observers.retain(|weak| weak.strong_count() > 0);
    }
}

fn valid_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REQUEST_ID
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-'))
}

fn exact_fields(object: &serde_json::Map<String, Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

fn decode_view(value: &Value) -> Result<PageView, FlowError> {
    let view = value
        .get("view")
        .ok_or_else(|| FlowError::Protocol("host frame missing view".into()))?;
    let bytes = serde_json::to_vec(view).map_err(|error| FlowError::Protocol(error.to_string()))?;
    PageView::decode(&bytes)
}

fn decode_snapshot(
    value: &Value,
    object: &serde_json::Map<String, Value>,
    kind: &str,
) -> Result<PageView, FlowError> {
    if !exact_fields(object, &["v", "request", "kind", "view", "ended"]) {
        return Err(FlowError::Protocol("invalid snapshot frame fields".into()));
    }
    let view = decode_view(value)?;
    let ended = value
        .get("ended")
        .and_then(Value::as_bool)
        .ok_or_else(|| FlowError::Protocol("snapshot frame missing terminal flag".into()))?;
    if (kind == "ready") != ended {
        return Err(FlowError::Protocol("invalid ready terminal flag".into()));
    }
    Ok(view)
}

pub(super) fn validate_snapshot_frame(value: &Value, kind: &str) -> Result<(), FlowError> {
    let object = value
        .as_object()
        .ok_or_else(|| FlowError::Protocol("host frame must be an object".into()))?;
    if value.get("v").and_then(Value::as_u64) != Some(u64::from(crate::WIRE_VERSION))
        || value.get("request") != Some(&Value::Null)
        || value.get("kind").and_then(Value::as_str) != Some(kind)
    {
        return Err(FlowError::Protocol("invalid snapshot frame".into()));
    }
    decode_snapshot(value, object, kind)?;
    Ok(())
}

fn parse_error(
    value: &Value,
    object: &serde_json::Map<String, Value>,
) -> Result<String, FlowError> {
    if !exact_fields(object, &["v", "request", "kind", "message"]) {
        return Err(FlowError::Protocol("invalid error frame fields".into()));
    }
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .filter(|message| !message.is_empty() && message.len() <= MAX_ERROR)
        .ok_or_else(|| FlowError::Protocol("invalid host error message".into()))?;
    Ok(message.into())
}

fn parse_run(value: &Value) -> Result<RunId, FlowError> {
    let run = value
        .get("run")
        .and_then(Value::as_str)
        .ok_or_else(|| FlowError::Protocol("host reply missing run ID".into()))?;
    serde_json::from_value(Value::String(run.into()))
        .map_err(|error| FlowError::Protocol(error.to_string()))
}

fn parse_scope(value: &Value) -> Result<metldr_history_pages::Scope, FlowError> {
    value
        .get("scope")
        .cloned()
        .ok_or_else(|| FlowError::Protocol("host reply missing scope".into()))
        .and_then(|scope| {
            serde_json::from_value(scope).map_err(|error| FlowError::Protocol(error.to_string()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::task::Waker;

    const STORE: &str = "0123456789abcdef0123456789abcdef";
    const SOURCE: &str = "0123456789abcdef0123456789abcdef:1";
    const RUN: &str = "0123456789abcdef0123456789abcdef:2";

    fn scope() -> metldr_history_pages::Scope {
        serde_json::from_value(
            json!({"store":STORE,"conversation":format!("{STORE}:100"),"epoch":"1"}),
        )
        .unwrap()
    }

    fn snapshot(sequence: u64, status: &str, output: &str) -> Value {
        json!({
            "sequence": sequence,
            "store": STORE,
            "selection": {"store":STORE,"conversation":format!("{STORE}:100"),"epoch":"1"},
            "source": {
                "id": SOURCE, "tab": 4, "document": "doc-1",
                "url": "http://127.0.0.1:1234/article", "title": "North report",
                "text": "The North project reports an amount of 17 units."
            },
            "runs": [{
                "id": RUN, "source": SOURCE, "kind": "summary", "question": null,
                "status": status, "output": output, "conversation": [], "error": null,
                "route": {"kind":"fixture"}, "activity": [], "corrections":{"total":0,"entries":[]}
            }],
            "native_correction": null,
            "pi_profile": null,
            "agent_sessions": []
        })
    }

    fn frame(request: Value, kind: &str, view: Value, ended: bool) -> Value {
        json!({"v": crate::WIRE_VERSION, "request": request, "kind": kind, "view": view, "ended": ended})
    }

    fn poll(cell: &Rc<RefCell<WatchCell>>) -> Poll<Result<Option<PageView>, FlowError>> {
        cell.borrow_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn correction_ack_requires_matching_scope_run_and_identity() {
        let mut protocol = Protocol::new();
        let cell = protocol
            .register_request(
                "correction-1".into(),
                ReplyKind::Correction(
                    scope(),
                    serde_json::from_value(json!(RUN)).unwrap(),
                    "Exact text".into(),
                ),
            )
            .unwrap();
        protocol
            .receive(json!({
                "v": crate::WIRE_VERSION,
                "request": "correction-1",
                "kind": "correction",
                "scope": scope(),
                "run": RUN,
                "receipt": {"request": "correction-1"}
            }))
            .unwrap();
        assert!(matches!(
            cell.borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));

        let mut protocol = Protocol::new();
        let cell = protocol
            .register_request(
                "correction-1".into(),
                ReplyKind::Correction(
                    scope(),
                    serde_json::from_value(json!(RUN)).unwrap(),
                    "Exact text".into(),
                ),
            )
            .unwrap();
        protocol
            .receive(json!({
                "v": crate::WIRE_VERSION,
                "request": "correction-1",
                "kind": "correction",
                "scope": scope(),
                "run": RUN,
                "receipt": {
                    "ordinal": 1,
                    "request": "correction-1",
                    "text": {"text":"Exact text","truncated":false},
                    "state": {"kind":"pending"}
                }
            }))
            .unwrap();
        assert!(matches!(
            cell.borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(Reply::Correction(_)))
        ));
    }

    #[test]
    fn capture_replies_and_live_views_keep_the_selected_scope() {
        let mut protocol = Protocol::new();
        let a = snapshot(1, "complete", "A");
        protocol
            .receive(frame(Value::Null, "ready", a.clone(), true))
            .unwrap();
        let scope: metldr_history_pages::Scope =
            serde_json::from_value(a["selection"].clone()).unwrap();
        let capture = protocol
            .register_request("capture-a".into(), ReplyKind::SelectedPage(scope))
            .unwrap();
        let mut b = snapshot(2, "complete", "B");
        b["selection"]["conversation"] = json!(format!("{STORE}:101"));
        b["selection"]["epoch"] = json!("2");
        protocol
            .receive(frame(Value::Null, "changed", b.clone(), false))
            .unwrap();
        protocol
            .receive(frame(json!("capture-a"), "view", b.clone(), true))
            .unwrap();
        assert!(matches!(
            capture
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));
        assert_eq!(protocol.latest.as_ref().unwrap().selection.epoch.0, 2);
        assert_eq!(protocol.latest.as_ref().unwrap().runs[0].output.text(), "B");
        let mut impossible = a;
        impossible["sequence"] = json!(3);
        assert!(
            protocol
                .receive(frame(Value::Null, "changed", impossible, false))
                .is_err()
        );
        let mut conflicting = b;
        conflicting["selection"]["conversation"] = json!(format!("{STORE}:102"));
        conflicting["sequence"] = json!(3);
        assert!(
            protocol
                .receive(frame(Value::Null, "changed", conflicting, false))
                .is_err()
        );
        assert_eq!(protocol.latest.as_ref().unwrap().selection.epoch.0, 2);
    }

    #[test]
    fn saved_history_details_preserve_legacy_ids_and_live_controls() {
        use crate::{
            history::{Context as HistoryContext, Expected, Response, Scope},
            model::RunView,
        };
        use metldr_history_pages::{Assembler, Counter, DocumentKind, Identity, Snapshot};
        let mut protocol = Protocol::new();
        protocol
            .receive(frame(
                Value::Null,
                "ready",
                snapshot(7, "running", "Live text"),
                true,
            ))
            .unwrap();
        let live: RunId = serde_json::from_value(json!(RUN)).unwrap();
        let context = HistoryContext {
            scope: Scope {
                store: STORE.into(),
                conversation: format!("{STORE}:100"),
                epoch: Counter(9_007_199_254_740_993),
            },
            generation: "owned-transport".into(),
        };
        let current = protocol
            .register_request(
                "history-current".into(),
                ReplyKind::History(Expected::Current),
            )
            .unwrap();
        protocol.receive(json!({"v":crate::WIRE_VERSION,"request":"history-current","kind":"history_current","scope":context.scope,"generation":context.generation})).unwrap();
        assert_eq!(
            current
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(Reply::History(Response::Current(context.clone()))))
        );
        let rows: Vec<_> = (65..=80).rev().map(|n| json!({"id":format!("{STORE}:{n:03}"),"source":format!("{STORE}:01"),"source_available":true,"revision":"1","kind":"summary","status":"complete","question":null,"output":{"text":"Retained text","truncated":false}})).collect();
        let mut list = json!({"v":crate::WIRE_VERSION,"request":"list","kind":"history_runs","generation":context.generation,"page":{"store":STORE,"conversation":context.scope.conversation,"epoch":context.scope.epoch,"sequence":"7","anchor":"100","runs":rows,"next":{"anchor":"100","before":"65"}}});
        let request = protocol
            .register_request(
                "list".into(),
                ReplyKind::History(Expected::Runs {
                    context: context.clone(),
                    cursor: None,
                }),
            )
            .unwrap();
        protocol.receive(list.clone()).unwrap();
        let Poll::Ready(Ok(Reply::History(Response::Runs(page)))) = request
            .borrow_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("expected stored summaries");
        };
        assert_eq!(page.runs.len(), 16);
        assert_eq!(page.runs[0].id.as_str(), format!("{STORE}:080"));
        assert_eq!(page.runs[0].source.as_str(), format!("{STORE}:01"));
        list["request"] = json!("oversized");
        list["page"]["runs"].as_array_mut().unwrap().push(json!({"id":format!("{STORE}:064"),"source":SOURCE,"source_available":true,"revision":"1","kind":"summary","status":"complete","question":null,"output":{"text":"older","truncated":false}}));
        let invalid = protocol
            .register_request(
                "oversized".into(),
                ReplyKind::History(Expected::Runs {
                    context: context.clone(),
                    cursor: None,
                }),
            )
            .unwrap();
        protocol.receive(list).unwrap();
        assert!(matches!(
            invalid
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));
        assert_eq!(protocol.latest.as_ref().unwrap().sequence, 7);

        let identity = Identity {
            store: STORE.into(),
            conversation: context.scope.conversation.clone(),
            epoch: context.scope.epoch,
            kind: DocumentKind::Run,
            document: page.runs[0].id.as_str().into(),
            revision: Counter(1),
            generation: context.generation.clone(),
            ticket: "owned-ticket:1".into(),
        };
        let output = format!("Saved \"text\" 🦀 {}", "x".repeat(7_000));
        let mut saved = snapshot(7, "complete", &output)["runs"][0].clone();
        saved["id"] = json!(identity.document);
        saved["source"] = json!(page.runs[0].source);
        let retained = Snapshot::new(identity.clone(), &saved).unwrap();
        let pin = protocol
            .register_request(
                "pin".into(),
                ReplyKind::History(Expected::Pin {
                    context: context.clone(),
                    kind: DocumentKind::Run,
                    document: identity.document.clone(),
                    revision: Counter(1),
                }),
            )
            .unwrap();
        protocol.receive(json!({"v":crate::WIRE_VERSION,"request":"pin","kind":"history_pinned","identity":identity})).unwrap();
        assert!(matches!(
            pin.borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(Reply::History(Response::Pinned(_))))
        ));
        let mut assembled = Assembler::new(identity.clone()).unwrap();
        let mut fragments = 0;
        loop {
            let offset = assembled.next_offset();
            let request = format!("fragment-{offset}");
            let cell = protocol
                .register_request(
                    request.clone(),
                    ReplyKind::History(Expected::Page {
                        identity: identity.clone(),
                        offset,
                    }),
                )
                .unwrap();
            protocol
                .receive(serde_json::from_slice(&retained.page(&request, offset).unwrap()).unwrap())
                .unwrap();
            let Poll::Ready(Ok(Reply::History(Response::Detail { request, bytes }))) = cell
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("expected detail");
            };
            fragments += 1;
            if fragments == 1 {
                let stop = protocol
                    .register_request("stop-live".into(), ReplyKind::Stop(scope(), live.clone()))
                    .unwrap();
                protocol.receive(json!({"v":crate::WIRE_VERSION,"request":"stop-live","kind":"stopping","scope":scope(),"run":live})).unwrap();
                assert_eq!(
                    stop.borrow_mut()
                        .poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Ok(Reply::Stop(StopReceipt::Requested)))
                );
                protocol
                    .receive(frame(Value::Null, "changed", snapshot_for_live(), false))
                    .unwrap();
                assert_eq!(
                    protocol.latest.as_ref().unwrap().runs[0].output.text(),
                    "Live update"
                );
            }
            if assembled.push(&bytes, &request).unwrap() {
                break;
            }
        }
        let run = RunView::decode_record(&assembled.finish().unwrap(), &page.runs[0].id).unwrap();
        assert!(fragments >= 4);
        assert_eq!(run.id.as_str(), identity.document);
        assert_eq!(run.source, page.runs[0].source);
        assert_eq!(run.output.text(), output);
        assert_eq!(protocol.latest.as_ref().unwrap().runs[0].id, live);
        assert_eq!(
            protocol.latest.as_ref().unwrap().runs[0].output.text(),
            "Live update"
        );
        let mut original = context.scope.clone();
        original.conversation = format!("{STORE}:101");
        original.epoch = Counter(context.scope.epoch.0 + 1);
        let mut current = context.scope.clone();
        current.epoch = Counter(context.scope.epoch.0 + 2);
        let receipt = json!({"v":crate::WIRE_VERSION,"request":"selection-retry","kind":"conversation_changed","original":original,"current":current,"generation":"after-switch","sequence":"8"});
        for _ in 0..2 {
            let cell = protocol
                .register_request(
                    "selection-retry".into(),
                    ReplyKind::History(Expected::Change {
                        scope: context.scope.clone(),
                        change: crate::history::Change::Create,
                    }),
                )
                .unwrap();
            protocol.receive(receipt.clone()).unwrap();
            let Poll::Ready(Ok(Reply::History(Response::Changed(change)))) = cell
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("expected the original and current selection receipt");
            };
            assert_eq!(change.original, original);
            assert_eq!(change.current.scope, current);
            assert_eq!(protocol.latest.as_ref().unwrap().sequence, 8);
            assert_eq!(protocol.latest.as_ref().unwrap().runs[0].id, live);
        }
        for id in [
            format!("{STORE}:0"),
            format!("{STORE}:-1"),
            format!("{STORE}:1 "),
            format!("{STORE}:{}1", "0".repeat(49_152)),
        ] {
            assert!(serde_json::from_value::<RunId>(json!(id)).is_err());
        }
        fn snapshot_for_live() -> Value {
            snapshot(8, "stop_requested", "Live update")
        }
    }

    #[test]
    fn correlates_replies_and_coalesces_watch_to_one_terminal_snapshot() {
        let mut protocol = Protocol::new();
        let failed = protocol
            .register_request("failed-start".into(), ReplyKind::Start(scope()))
            .unwrap();
        protocol
            .receive(json!({"v":crate::WIRE_VERSION,"request":"failed-start","kind":"error","message":"reply failed after commit"}))
            .unwrap();
        assert!(matches!(
            failed
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Remote(message))) if message == "reply failed after commit"
        ));
        assert!(
            protocol
                .receive(json!({"v":crate::WIRE_VERSION,"request":7,"kind":"started","run":RUN}))
                .is_err()
        );
        let reply = protocol
            .register_request("start-1".into(), ReplyKind::Start(scope()))
            .unwrap();
        let watch = protocol
            .register_watch(
                "watch-1".into(),
                scope(),
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        protocol
            .receive(
                json!({"v":crate::WIRE_VERSION,"request":"foreign","kind":"started","scope":scope(),"run":RUN}),
            )
            .unwrap();
        assert!(matches!(
            reply
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        protocol
            .receive(
                json!({"v":crate::WIRE_VERSION,"request":"start-1","kind":"started","scope":scope(),"run":RUN}),
            )
            .unwrap();
        assert!(matches!(
            reply
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(Reply::Started(_)))
        ));

        protocol
            .receive(frame(
                json!("watch-1"),
                "view",
                snapshot(1, "running", "first"),
                false,
            ))
            .unwrap();
        protocol
            .receive(frame(
                json!("watch-1"),
                "view",
                snapshot(2, "complete", "final"),
                true,
            ))
            .unwrap();
        match poll(&watch) {
            Poll::Ready(Ok(Some(view))) => {
                assert_eq!(view.sequence, 2);
                assert_eq!(view.runs[0].output.text(), "final");
            }
            other => panic!("expected coalesced terminal snapshot, got {other:?}"),
        }
        protocol
            .receive(frame(
                json!("watch-1"),
                "view",
                snapshot(3, "complete", "late"),
                true,
            ))
            .unwrap();
        assert!(matches!(poll(&watch), Poll::Ready(Ok(None))));

        let bad_watch = protocol
            .register_watch(
                "bad-watch".into(),
                scope(),
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        protocol
            .receive(frame(
                json!("bad-watch"),
                "view",
                snapshot(4, "running", "still active"),
                true,
            ))
            .unwrap();
        assert!(matches!(
            poll(&bad_watch),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));
        assert!(matches!(poll(&bad_watch), Poll::Ready(Ok(None))));
    }

    #[test]
    fn rejects_scope_mismatches_for_start_stop_and_watch() {
        let expected = scope();
        let other: metldr_history_pages::Scope = serde_json::from_value(json!({
            "store": STORE,
            "conversation": format!("{STORE}:101"),
            "epoch": "2"
        }))
        .unwrap();
        let mut protocol = Protocol::new();

        let start = protocol
            .register_request("scoped-start".into(), ReplyKind::Start(expected.clone()))
            .unwrap();
        protocol
            .receive(json!({
                "v": crate::WIRE_VERSION,
                "request": "scoped-start",
                "kind": "started",
                "scope": other,
                "run": RUN
            }))
            .unwrap();
        assert!(matches!(
            start
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));

        let stop = protocol
            .register_request(
                "scoped-stop".into(),
                ReplyKind::Stop(
                    expected.clone(),
                    serde_json::from_value(json!(RUN)).unwrap(),
                ),
            )
            .unwrap();
        let other: metldr_history_pages::Scope = serde_json::from_value(json!({
            "store": STORE,
            "conversation": format!("{STORE}:101"),
            "epoch": "2"
        }))
        .unwrap();
        protocol
            .receive(json!({
                "v": crate::WIRE_VERSION,
                "request": "scoped-stop",
                "kind": "stopping",
                "scope": other,
                "run": RUN
            }))
            .unwrap();
        assert!(matches!(
            stop.borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));

        let mut initial = snapshot(1, "complete", "initial");
        protocol
            .receive(frame(Value::Null, "ready", initial.clone(), true))
            .unwrap();
        let watch = protocol
            .register_watch(
                "scoped-watch".into(),
                expected,
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        initial["selection"]["conversation"] = json!(format!("{STORE}:101"));
        initial["selection"]["epoch"] = json!("2");
        initial["sequence"] = json!(2);
        protocol
            .receive(frame(json!("scoped-watch"), "view", initial, true))
            .unwrap();
        assert_eq!(protocol.latest.as_ref().unwrap().sequence, 1);
        assert!(matches!(
            poll(&watch),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));
    }

    #[test]
    fn disconnect_fails_waiters_observers_reconnect_and_removal_keeps_run() {
        let mut protocol = Protocol::new();
        protocol
            .receive(frame(
                Value::Null,
                "ready",
                snapshot(1, "running", "partial"),
                true,
            ))
            .unwrap();
        let observer = protocol.observe().unwrap();
        assert!(matches!(poll(&observer), Poll::Ready(Ok(Some(_)))));
        let wrong_stop = protocol
            .register_request(
                "wrong-stop".into(),
                ReplyKind::Stop(scope(), serde_json::from_value(json!(RUN)).unwrap()),
            )
            .unwrap();
        protocol
            .receive(json!({"v":crate::WIRE_VERSION,"request":"wrong-stop","kind":"stopping","scope":scope(),"run":"0123456789abcdef0123456789abcdef:3"}))
            .unwrap();
        assert!(matches!(
            wrong_stop
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Protocol(_)))
        ));
        let reply = protocol
            .register_request(
                "stop-1".into(),
                ReplyKind::Stop(scope(), serde_json::from_value(json!(RUN)).unwrap()),
            )
            .unwrap();
        let terminal_watch = protocol
            .register_watch(
                "terminal-watch".into(),
                scope(),
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        protocol
            .receive(frame(
                json!("terminal-watch"),
                "view",
                snapshot(2, "complete", "terminal"),
                true,
            ))
            .unwrap();
        let watch = protocol
            .register_watch(
                "watch-1".into(),
                scope(),
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();

        protocol
            .receive(json!({"v":crate::WIRE_VERSION,"request":null,"kind":"disconnected"}))
            .unwrap();
        assert!(matches!(
            reply
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(FlowError::Disconnected))
        ));
        assert!(matches!(
            poll(&watch),
            Poll::Ready(Err(FlowError::Disconnected))
        ));
        assert!(matches!(poll(&watch), Poll::Ready(Ok(None))));
        assert!(matches!(
            poll(&terminal_watch),
            Poll::Ready(Ok(Some(view))) if view.sequence == 2
        ));
        assert!(matches!(poll(&terminal_watch), Poll::Ready(Ok(None))));
        assert!(terminal_watch.borrow().terminal_received());
        assert!(!watch.borrow().terminal_received());
        assert!(matches!(
            poll(&observer),
            Poll::Ready(Err(FlowError::Disconnected))
        ));

        protocol
            .receive(frame(
                Value::Null,
                "ready",
                snapshot(2, "complete", "terminal"),
                true,
            ))
            .unwrap();
        assert!(protocol.connected);
        assert!(matches!(poll(&observer), Poll::Ready(Ok(Some(view))) if view.sequence == 2));
        protocol
            .receive(frame(
                Value::Null,
                "changed",
                snapshot(3, "running", "three"),
                false,
            ))
            .unwrap();
        protocol
            .receive(frame(
                Value::Null,
                "changed",
                snapshot(4, "running", "four"),
                false,
            ))
            .unwrap();
        assert!(matches!(poll(&observer), Poll::Ready(Ok(Some(view))) if view.sequence == 4));
        assert!(matches!(poll(&observer), Poll::Pending));

        let removed = protocol
            .register_watch(
                "watch-2".into(),
                scope(),
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        protocol.remove_watch("watch-2");
        assert!(matches!(poll(&removed), Poll::Ready(Ok(None))));
        assert!(!removed.borrow().terminal_received());
        assert_eq!(
            protocol
                .latest
                .as_ref()
                .unwrap()
                .active_run()
                .unwrap()
                .as_str(),
            RUN
        );
        assert!(protocol.connected);
        protocol.disconnect(FlowError::Disconnected, false);
        protocol.disconnect(FlowError::Disconnected, true);
        assert!(matches!(
            poll(&observer),
            Poll::Ready(Err(FlowError::Disconnected))
        ));
        assert!(matches!(poll(&observer), Poll::Ready(Ok(None))));
        protocol
            .receive(frame(
                Value::Null,
                "ready",
                snapshot(5, "running", "late"),
                true,
            ))
            .unwrap();
        assert!(!protocol.connected);
        assert!(protocol.closed);
    }
}
