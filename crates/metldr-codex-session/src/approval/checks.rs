use super::*;

#[test]
fn native_choices_keep_exact_responses_and_invalidate_old_handles() {
    let turn = TurnId("turn1".into());
    let params = json!({"itemId":"same-item","availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["cmd","arg"]}},"cancel"]});
    let mut pending = Approvals::default();
    let old = pending
        .insert(
            1,
            &turn,
            json!(0),
            "item/commandExecution/requestApproval",
            &params,
            None,
        )
        .unwrap();
    assert_eq!(old.choices.len(), 3);
    assert_eq!(
        pending.response(old.id(), &turn, Some("1")).unwrap().1["result"]["decision"],
        params["availableDecisions"][1]
    );
    assert!(pending.response(old.id(), &turn, Some("decline")).is_err());
    assert!(
        pending
            .response(old.id(), &TurnId("other".into()), Some("0"))
            .is_err()
    );
    assert_eq!(pending.resolved(&json!(0)), Some(old.id.clone()));
    let fresh = pending
        .insert(
            1,
            &turn,
            json!(0),
            "item/commandExecution/requestApproval",
            &params,
            None,
        )
        .unwrap();
    assert_ne!(old.id(), fresh.id());
    assert!(pending.response(old.id(), &turn, Some("0")).is_err());
    let file = pending
        .insert(
            1,
            &turn,
            json!(1),
            "item/fileChange/requestApproval",
            &json!({"itemId":"file1"}),
            Some(json!([{"diff":"original"}])),
        )
        .unwrap();
    let changed = pending
        .refresh("file1", json!([{"diff":"updated"}]))
        .unwrap();
    assert_eq!(changed.len(), 1);
    assert!(pending.response(file.id(), &turn, Some("0")).is_err());
    assert_eq!(changed[0].1.review()["changes"][0]["diff"], "updated");
    pending.clear();
    assert!(pending.response(fresh.id(), &turn, Some("0")).is_err());
}
