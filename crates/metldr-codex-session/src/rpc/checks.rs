use super::*;
use crate::{
    CodexSession, Event, SteerOutcome, ThreadId, ThreadInfo, TurnId, TurnMode, approval, input,
    report,
};

fn rpc(mut child: Child) -> Rpc {
    Rpc {
        wire: Wire {
            stdin: child.stdin.take(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            frame: Vec::new(),
            queue: VecDeque::new(),
            queued_bytes: 0,
            sequence: 0,
            pending: None,
        },
        child,
        healthy: true,
    }
}

fn peer(script: &str) -> Child {
    Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

fn session(script: &str) -> CodexSession {
    CodexSession {
        rpc: rpc(peer(script)),
        info: ThreadInfo {
            id: ThreadId("thread-1".into()),
            session_id: "session-1".into(),
            model: "model".into(),
            provider: "provider".into(),
            cwd: "/workspace".into(),
        },
        turn: Some(TurnId("turn-1".into())),
        connection: 1,
        pending_tools: Vec::new(),
        reports: report::Reports::default(),
        approvals: approval::Approvals::default(),
        inputs: input::Inputs::default(),
        steering: None,
        failed: false,
    }
}

async fn assert_killed(child: &mut Child) {
    let status = timeout(Duration::from_secs(1), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success());
}

#[tokio::test(flavor = "current_thread")]
async fn steer_reply_is_delivered_after_interrupt_in_either_reply_order() {
    for steer_first in [true, false] {
        let script = if steer_first {
            "read -r steer; read -r interrupt; case \"$interrupt\" in *turn/interrupt*) ;; *) exit 9;; esac; printf '{\"id\":1,\"result\":{\"turnId\":\"turn-1\"}}\\n'; printf '{\"id\":2,\"result\":{}}\\n'"
        } else {
            "read -r steer; read -r interrupt; case \"$interrupt\" in *turn/interrupt*) ;; *) exit 9;; esac; printf '{\"id\":2,\"result\":{}}\\n'; printf '{\"id\":1,\"result\":{\"turnId\":\"turn-1\"}}\\n'"
        };
        let mut session = session(script);
        let turn = TurnId("turn-1".into());
        let request = session.steer(&turn, "correction").await.unwrap();
        assert!(
            session
                .prompt("new prompt", TurnMode::Configured)
                .await
                .is_err()
        );
        assert_eq!(
            session.interrupt().await.unwrap(),
            crate::Interrupt::Acknowledged
        );
        match session.next_event().await.unwrap() {
            Event::SteerResult {
                request: received,
                outcome,
            } => {
                assert_eq!(received, request);
                assert_eq!(outcome, SteerOutcome::Accepted { turn });
            }
            _ => panic!("expected steering receipt"),
        }
    }
    let mut session = session(
        "read -r steer; read -r interrupt; printf '%s\\n' '{\"id\":1,\"result\":{\"turnId\":\"turn-1\"}}'; sleep 0.4; printf '%s\\n' '{\"id\":2,\"result\":{}}'",
    );
    let turn = TurnId("turn-1".into());
    let request = session.steer(&turn, "correction").await.unwrap();
    session.rpc.wire.pending.as_mut().unwrap().deadline =
        Instant::now() + Duration::from_millis(200);
    assert_eq!(
        session.interrupt().await.unwrap(),
        crate::Interrupt::Acknowledged
    );
    assert!(
        matches!(session.next_event().await.unwrap(), Event::SteerResult { request:received, .. } if received == request)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn steer_receipt_keeps_old_turn_after_terminal_event() {
    for rejected in [false, true] {
        let reply = if rejected {
            "{\"id\":1,\"error\":{\"code\":-1,\"message\":\"rejected\"}}"
        } else {
            "{\"id\":1,\"result\":{\"turnId\":\"turn-1\"}}"
        };
        let script = format!(
            "read -r steer; read -r interrupt; case \"$interrupt\" in *turn/interrupt*) ;; *) exit 9;; esac; printf '%s\\n' '{{\"method\":\"turn/completed\",\"params\":{{\"threadId\":\"thread-1\",\"turn\":{{\"id\":\"turn-1\",\"status\":\"interrupted\"}}}}}}'; printf '%s\\n' '{reply}'; printf '{{\"id\":2,\"result\":{{}}}}\\n'"
        );
        let mut session = session(&script);
        let turn = TurnId("turn-1".into());
        let request = session.steer(&turn, "correction").await.unwrap();
        assert!(
            session
                .prompt("new prompt", TurnMode::Configured)
                .await
                .is_err()
        );
        assert_eq!(
            session.interrupt().await.unwrap(),
            crate::Interrupt::Acknowledged
        );
        assert!(
            matches!(session.next_event().await.unwrap(), Event::Finished { turn: ended, .. } if ended == turn)
        );
        match session.next_event().await.unwrap() {
            Event::SteerResult {
                request: received,
                outcome,
            } => {
                assert_eq!(received, request);
                assert_eq!(
                    outcome,
                    if rejected {
                        SteerOutcome::Rejected { code: -1 }
                    } else {
                        SteerOutcome::Accepted { turn }
                    }
                );
            }
            _ => panic!("expected steering receipt"),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_steer_replies_and_expired_or_blocked_io_poison_the_peer() {
    for reply in [
        "{\"id\":99,\"result\":{\"turnId\":\"turn-1\"}}",
        "{\"id\":1,\"error\":{\"code\":-1}}",
        "{\"id\":1,\"result\":{\"turnId\":\"other-turn\"}}",
        "{\"id\":1,\"result\":{},\"error\":{\"code\":-1,\"message\":\"rejected\"}}",
        "{\"id\":1,\"result\":{\"turnId\":\"turn-1\"}}\\n{\"id\":1,\"result\":{\"turnId\":\"turn-1\"}}",
    ] {
        let script = format!("read -r steer; printf '%b\\n' '{reply}'; exec sleep 60");
        let mut session = session(&script);
        session
            .steer(&TurnId("turn-1".into()), "correction")
            .await
            .unwrap();
        if reply.contains("\\n{") {
            assert!(matches!(
                session.next_event().await.unwrap(),
                Event::SteerResult { .. }
            ));
        }
        assert!(session.next_event().await.is_err());
        assert!(session.failed);
        assert_killed(&mut session.rpc.child).await;
    }

    let mut session = session("read -r steer; exec sleep 60");
    session
        .steer(&TurnId("turn-1".into()), "correction")
        .await
        .unwrap();
    session.rpc.wire.pending.as_mut().unwrap().deadline =
        Instant::now() + Duration::from_millis(20);
    assert!(session.next_event().await.is_err());
    assert!(session.failed);
    assert_killed(&mut session.rpc.child).await;

    let mut rpc = rpc(peer("exec sleep 60"));
    assert!(
        rpc.begin_steer(json!({"held":"x".repeat(900_000)}))
            .await
            .is_err()
    );
    assert!(!rpc.healthy);
    assert!(rpc.begin_steer(json!({})).await.is_err());
    assert_killed(&mut rpc.child).await;
}

#[tokio::test(flavor = "current_thread")]
async fn clean_stdout_eof_allows_normal_process_close() {
    let mut rpc = rpc(peer("exec 1>&-; read -r ignored || :"));
    let error = rpc.next().await.unwrap_err();
    assert!(clean_eof(error.as_ref()));
    let exit = rpc.close().await.unwrap();
    assert_eq!(exit.code, Some(0));
    assert!(!exit.forced && exit.drained);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_rpc_and_blocked_write_kill_child_and_poison_connection() {
    for blocked_write in [false, true] {
        let mut rpc = rpc(peer("exec sleep 60"));
        let deadline = Duration::from_millis(30);
        if blocked_write {
            assert!(
                timeout(deadline, rpc.send(json!({"held":"x".repeat(900_000)})))
                    .await
                    .is_err()
            );
        } else {
            assert!(
                timeout(deadline, rpc.call("held", json!({})))
                    .await
                    .is_err()
            );
        }
        assert!(!rpc.healthy);
        assert!(rpc.call("later", json!({})).await.is_err());
        assert_killed(&mut rpc.child).await;
    }
}
