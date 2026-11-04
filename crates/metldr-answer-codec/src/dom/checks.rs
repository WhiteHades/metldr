use super::*;
use serde_json::{Value, json};

fn binding() -> Binding {
    Binding::new(
        "1".repeat(32),
        "thread".into(),
        "run".into(),
        "answer".into(),
    )
    .unwrap()
}

fn document(parents: Vec<u32>) -> (Rc<Cell<DocumentState>>, DomSession) {
    let owner = Rc::new(Cell::new(DocumentState::Ready(NonZeroU32::new(7).unwrap())));
    let session = DomSession::mount(binding(), &owner, parents).unwrap();
    (owner, session)
}

fn packet(sequence: u32, operations: Value) -> Value {
    json!({"binding": binding(), "documentId": 7, "sequence": sequence, "operations": operations})
}

#[test]
fn idle_documents_can_be_probed_until_their_owner_closes() {
    let (_owner, mut dom) = document(vec![0]);
    assert_eq!(dom.task_count(), 0);
    assert_eq!(dom.probe().unwrap(), 1);
    let pong = json!({"binding":binding(),"documentId":7,"task":{"kind":"pong","id":1}});
    assert_eq!(
        dom.receive_task(pong.to_string()).unwrap().kind(),
        TaskEventKind::Pong
    );
    dom.close();
    assert!(dom.probe().is_err());
}

#[test]
fn asynchronous_tasks_and_probes_stay_with_their_document() {
    let (_owner, mut dom) = document(vec![0, 1]);
    let packet = |kind: &str, id: u32| json!({"binding":binding(),"documentId":7,"task":{"kind":kind,"id":id}});
    assert!(dom.receive_task(packet("started", 0).to_string()).is_err());
    for id in 1..=32 {
        assert_eq!(
            dom.receive_task(packet("started", id).to_string())
                .unwrap()
                .kind(),
            TaskEventKind::Started
        );
    }
    assert_eq!(dom.task_count(), 32);
    assert!(dom.receive_task(packet("started", 33).to_string()).is_err());
    assert!(dom.receive_task(packet("started", 32).to_string()).is_err());
    dom.receive_task(packet("finished", 1).to_string()).unwrap();
    assert!(dom.receive_task(packet("finished", 1).to_string()).is_err());
    dom.receive_task(packet("started", 33).to_string()).unwrap();
    assert!(dom.receive_task(packet("pong", 1).to_string()).is_err());
    assert_eq!(dom.probe().unwrap(), 1);
    assert_eq!(dom.probe().unwrap(), 2);
    assert_eq!(
        dom.receive_task(packet("pong", 1).to_string())
            .unwrap()
            .kind(),
        TaskEventKind::Ignored
    );
    assert_eq!(
        dom.receive_task(packet("pong", 2).to_string())
            .unwrap()
            .kind(),
        TaskEventKind::Pong
    );
    assert_eq!(
        dom.receive_task(packet("pong", 2).to_string())
            .unwrap()
            .kind(),
        TaskEventKind::Ignored
    );
    let mut foreign = packet("failed", 2);
    foreign["documentId"] = json!(8);
    assert!(dom.receive_task(foreign.to_string()).is_err());
    let mut foreign = packet("failed", 2);
    foreign["binding"]["frameId"] = json!("2".repeat(32));
    assert!(dom.receive_task(foreign.to_string()).is_err());
    let mut extra = packet("failed", 2);
    extra["task"]["extra"] = json!(true);
    assert!(dom.receive_task(extra.to_string()).is_err());
    assert_eq!(dom.task_count(), 32);
    assert_eq!(
        dom.receive_task(packet("failed", 2).to_string())
            .unwrap()
            .kind(),
        TaskEventKind::Failed
    );
    dom.close();
    assert!(dom.receive_task(packet("started", 34).to_string()).is_err());
    assert!(dom.probe().is_err());
}

#[test]
fn only_the_current_document_accepts_ordered_batches() {
    let (_owner, mut dom) = document(vec![0, 1]);
    let valid = packet(1, json!([{"kind":"setText", "node":2, "text":"ready"}]));
    for field in ["documentId", "sequence"] {
        let mut stale = valid.clone();
        stale[field] = json!(8);
        assert!(dom.receive(stale.to_string()).is_err());
    }
    let mut other_answer = valid.clone();
    other_answer["binding"]["answerId"] = json!("another answer");
    assert!(dom.receive(other_answer.to_string()).is_err());
    let mut other_frame = valid.clone();
    other_frame["binding"]["frameId"] = json!("2".repeat(32));
    assert!(dom.receive(other_frame.to_string()).is_err());
    let mut batch = dom.receive(valid.to_string()).unwrap();
    let mutation = batch.next().unwrap();
    assert_eq!(mutation.kind(), DomMutationKind::SetText);
    assert_eq!(mutation.node(), 2);
    assert_eq!(mutation.text().unwrap(), "ready");
    assert!(batch.next().is_none());
    assert!(dom.receive(valid.to_string()).is_err());
    dom.close();
    assert!(dom.receive(packet(2, json!([])).to_string()).is_err());
    assert!(
        dom.event(2, DomEventKind::Input, Some("ignored".into()))
            .is_err()
    );

    #[cfg(feature = "guest")]
    {
        let initialization = crate::encode_initialize(&binding()).unwrap();
        let mut guest = crate::GuestSession::new(initialization).unwrap();
        let encoded = crate::encode_host(
            &binding(),
            crate::HostMessage::Document {
                request_id: NonZeroU32::new(7).unwrap(),
                css: String::new(),
                html: "<p>ready</p>".into(),
            },
        )
        .unwrap();
        guest.receive(encoded).unwrap();
        let mut mounted = guest.mount_dom(vec![0, 1]).unwrap();
        assert!(mounted.receive(valid.to_string()).is_ok());
        let encoded = crate::encode_host(
            &binding(),
            crate::HostMessage::Document {
                request_id: NonZeroU32::new(8).unwrap(),
                css: String::new(),
                html: "<p>new document</p>".into(),
            },
        )
        .unwrap();
        guest.receive(encoded).unwrap();
        let mut replacement = guest.mount_dom(vec![0, 1]).unwrap();
        assert!(mounted.receive(packet(2, json!([])).to_string()).is_err());
        drop(guest);
        let mut after_drop = packet(1, json!([]));
        after_drop["documentId"] = json!(8);
        assert!(replacement.receive(after_drop.to_string()).is_err());
    }
}

#[test]
fn rejected_batches_leave_nodes_listeners_and_sequences_unchanged() {
    let (_owner, mut dom) = document(vec![0, 1, 2, 1]);
    let rejected = packet(
        1,
        json!([
            {"kind":"listen", "node":3, "event":"input"},
            {"kind":"setText", "node":2, "text":"replace"},
            {"kind":"setValue", "node":3, "value":"removed"}
        ]),
    );
    assert!(dom.receive(rejected.to_string()).is_err());
    assert!(
        dom.event(3, DomEventKind::Input, Some("unexpected".into()))
            .is_err()
    );
    dom.receive(
        packet(
            1,
            json!([
                {"kind":"listen", "node":3, "event":"input"},
                {"kind":"listen", "node":4, "event":"click"}
            ]),
        )
        .to_string(),
    )
    .unwrap();
    let event: Value = serde_json::from_str(
        &dom.event(3, DomEventKind::Input, Some("四".into()))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        event,
        json!({"binding":binding(), "documentId":7, "sequence":1, "node":3, "kind":"input", "value":"四"})
    );
    dom.receive(
        packet(
            2,
            json!([{"kind":"setText", "node":2, "text":"replacement"}]),
        )
        .to_string(),
    )
    .unwrap();
    assert!(
        dom.event(3, DomEventKind::Input, Some("late".into()))
            .is_err()
    );
    assert!(
        dom.receive(packet(3, json!([{"kind":"setValue", "node":3, "value":"late"}])).to_string())
            .is_err()
    );
    let sibling: Value =
        serde_json::from_str(&dom.event(4, DomEventKind::Click, None).unwrap()).unwrap();
    assert_eq!(sibling["sequence"], 2);
    dom.receive(
        packet(
            3,
            json!([{"kind":"setDisabled", "node":4, "disabled":true}]),
        )
        .to_string(),
    )
    .unwrap();
}

#[test]
fn boundary_limits_and_closed_operation_set_are_enforced() {
    let owner = Rc::new(Cell::new(DocumentState::Ready(NonZeroU32::new(7).unwrap())));
    assert!(DomSession::mount(binding(), &owner, vec![]).is_err());
    assert!(DomSession::mount(binding(), &owner, vec![0; MAX_NODES + 1]).is_err());
    assert!(DomSession::mount(binding(), &owner, vec![0, 2]).is_err());
    let (_owner, mut dom) = document(vec![0, 1]);
    let listen = json!({"kind":"listen", "node":2, "event":"input"});
    for operations in [
        json!([{"kind":"listen", "node":0, "event":"input"}]),
        json!([{"kind":"listen", "node":3, "event":"input"}]),
        json!([{"kind":"listen", "node":2, "event":"submit"}]),
        json!([{"kind":"listen", "node":2, "event":"input", "extra":true}]),
        json!([{"kind":"setAttribute", "node":2, "name":"onclick", "value":"code"}]),
        json!([{"kind":"setDataset", "node":2, "key":"bad-key", "value":"x"}]),
        json!([{"kind":"setText", "node":2, "text":"界".repeat(MAX_TEXT_BYTES / 3 + 1)}]),
        json!(vec![listen.clone(); MAX_OPERATIONS + 1]),
    ] {
        assert!(dom.receive(packet(1, operations).to_string()).is_err());
    }
    assert!(dom.receive(" ".repeat(MAX_BATCH_BYTES + 1)).is_err());
    let mut extra = packet(1, json!([]));
    extra["extra"] = json!(true);
    assert!(dom.receive(extra.to_string()).is_err());
    let mut batch = dom
        .receive(
            packet(
                1,
                json!([
                    listen,
                    {"kind":"setValue", "node":2, "value":"4"},
                    {"kind":"setDataset", "node":2, "key":"order", "value":"async,next"},
                    {"kind":"setDisabled", "node":2, "disabled":false}
                ]),
            )
            .to_string(),
        )
        .unwrap();
    assert_eq!(
        batch.next().unwrap().event_kind().unwrap(),
        DomEventKind::Input
    );
    assert_eq!(batch.next().unwrap().text().unwrap(), "4");
    let dataset = batch.next().unwrap();
    assert_eq!(dataset.key().unwrap(), "order");
    assert_eq!(dataset.text().unwrap(), "async,next");
    assert!(!batch.next().unwrap().disabled().unwrap());
    assert!(
        dom.event(
            2,
            DomEventKind::Input,
            Some("界".repeat(MAX_EVENT_BYTES / 3 + 1))
        )
        .is_err()
    );
    assert!(dom.event(2, DomEventKind::Input, None).is_err());
    assert!(
        dom.event(2, DomEventKind::Click, Some("bad".into()))
            .is_err()
    );
    let event: Value = serde_json::from_str(
        &dom.event(2, DomEventKind::Input, Some(String::new()))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(event["sequence"], 1);
}
