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
    Page,
    Start,
    Stop(RunId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Page(PageView),
    Started(RunId),
    Stop(StopReceipt),
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
    run: Option<RunId>,
    queue: Option<PageView>,
    terminal_queued: bool,
    last_observed: Option<(String, u64)>,
    error: Option<FlowError>,
    closed: bool,
    close_after_error: bool,
    waker: Option<Waker>,
}

impl WatchCell {
    fn specific(run: RunId) -> Self {
        Self {
            run: Some(run),
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
            run: None,
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
            if self.run.is_some() && self.terminal_queued {
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
        if self.closed || self.run.is_some() {
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
        let run = self
            .run
            .as_ref()
            .ok_or_else(|| FlowError::Protocol("view response was routed to an observer".into()))?;
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
    watches: BTreeMap<String, (RunId, Weak<RefCell<WatchCell>>)>,
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
        run: RunId,
    ) -> Result<Rc<RefCell<WatchCell>>, FlowError> {
        self.prune();
        self.validate_registration(&id)?;
        if self.watches.len() >= MAX_WATCHES || self.watches.contains_key(&id) {
            return Err(FlowError::Protocol("watch capacity or ID conflict".into()));
        }
        let cell = Rc::new(RefCell::new(WatchCell::specific(run.clone())));
        self.watches.insert(id, (run, Rc::downgrade(&cell)));
        Ok(cell)
    }

    pub(crate) fn remove_watch(&mut self, id: &str) {
        if let Some((_, cell)) = self.watches.remove(id) {
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
        if version != Some(4) || request.is_none() || kind.is_none() {
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
            if let Some((_, cell)) = self.watches.get(request_id).cloned() {
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
            if expected != ReplyKind::Page {
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
                .and_then(|cell| cell.borrow_mut().fail(FlowError::Host(message)))
                .into_iter()
                .collect());
        }

        let Some((expected, weak)) = self.pending.get(request_id).cloned() else {
            return Ok(Vec::new());
        };
        let (reply, fields) = match (expected, kind) {
            (ReplyKind::Start, "started") => match parse_run(&value) {
                Ok(run) => (Reply::Started(run), &["v", "request", "kind", "run"][..]),
                Err(error) => return self.fail_request(request_id, error),
            },
            (ReplyKind::Stop(expected_run), "stopping") => match parse_run(&value) {
                Ok(run) if run == expected_run => (
                    Reply::Stop(StopReceipt::Requested),
                    &["v", "request", "kind", "run"][..],
                ),
                Ok(_) => {
                    return self.fail_request(
                        request_id,
                        FlowError::Protocol("stop receipt run does not match request".into()),
                    );
                }
                Err(error) => return self.fail_request(request_id, error),
            },
            (ReplyKind::Stop(expected_run), "already_terminal") => match parse_run(&value) {
                Ok(run) if run == expected_run => (
                    Reply::Stop(StopReceipt::AlreadyTerminal),
                    &["v", "request", "kind", "run"][..],
                ),
                Ok(_) => {
                    return self.fail_request(
                        request_id,
                        FlowError::Protocol("stop receipt run does not match request".into()),
                    );
                }
                Err(error) => return self.fail_request(request_id, error),
            },
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
        for (_, weak) in watches.into_values() {
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

    fn update_view(&mut self, view: PageView, reconnect: bool) -> Vec<Waker> {
        let accept = self
            .latest
            .as_ref()
            .is_none_or(|latest| latest.store != view.store || latest.sequence < view.sequence);
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
        let Some((_, weak)) = self.watches.remove(id) else {
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
        self.watches.retain(|_, (_, weak)| weak.strong_count() > 0);
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
    if value.get("v").and_then(Value::as_u64) != Some(4)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::task::Waker;

    const STORE: &str = "0123456789abcdef0123456789abcdef";
    const SOURCE: &str = "0123456789abcdef0123456789abcdef:1";
    const RUN: &str = "0123456789abcdef0123456789abcdef:2";

    fn snapshot(sequence: u64, status: &str, output: &str) -> Value {
        json!({
            "sequence": sequence,
            "store": STORE,
            "source": {
                "id": SOURCE, "tab": 4, "document": "doc-1",
                "url": "http://127.0.0.1:1234/article", "title": "North report",
                "text": "The North project reports an amount of 17 units."
            },
            "runs": [{
                "id": RUN, "source": SOURCE, "kind": "summary", "question": null,
                "status": status, "output": output, "conversation": [], "error": null,
                "route": {"kind":"fixture"}, "activity": []
            }],
            "pi_profile": null,
            "agent_sessions": []
        })
    }

    fn frame(request: Value, kind: &str, view: Value, ended: bool) -> Value {
        json!({"v": 4, "request": request, "kind": kind, "view": view, "ended": ended})
    }

    fn poll(cell: &Rc<RefCell<WatchCell>>) -> Poll<Result<Option<PageView>, FlowError>> {
        cell.borrow_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn correlates_replies_and_coalesces_watch_to_one_terminal_snapshot() {
        let mut protocol = Protocol::new();
        assert!(
            protocol
                .receive(json!({"v":4,"request":7,"kind":"started","run":RUN}))
                .is_err()
        );
        let reply = protocol
            .register_request("start-1".into(), ReplyKind::Start)
            .unwrap();
        let watch = protocol
            .register_watch(
                "watch-1".into(),
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        protocol
            .receive(json!({"v":4,"request":"foreign","kind":"started","run":RUN}))
            .unwrap();
        assert!(matches!(
            reply
                .borrow_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        protocol
            .receive(json!({"v":4,"request":"start-1","kind":"started","run":RUN}))
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
                ReplyKind::Stop(serde_json::from_value(json!(RUN)).unwrap()),
            )
            .unwrap();
        protocol
            .receive(json!({"v":4,"request":"wrong-stop","kind":"stopping","run":"0123456789abcdef0123456789abcdef:3"}))
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
                ReplyKind::Stop(serde_json::from_value(json!(RUN)).unwrap()),
            )
            .unwrap();
        let terminal_watch = protocol
            .register_watch(
                "terminal-watch".into(),
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
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();

        protocol
            .receive(json!({"v":4,"request":null,"kind":"disconnected"}))
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
                serde_json::from_value(json!(RUN)).unwrap(),
            )
            .unwrap();
        protocol.remove_watch("watch-2");
        assert!(matches!(poll(&removed), Poll::Ready(Ok(None))));
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
