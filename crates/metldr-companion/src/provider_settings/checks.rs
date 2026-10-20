use super::*;
use crate::{https_profile::Profile, model::Store};
use metldr_native_credentials::{CredentialId, Error, Key, Vault};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant},
};

const EXTENSION: &str = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";

struct Host {
    child: Child,
    rx: Receiver<Value>,
}
impl Host {
    fn start(config: &Path) -> Self {
        let mut child = Command::new("/binaries/metldr-companion")
            .arg(EXTENSION)
            .env("METLDR_FIRST_FLOW_CONFIG", config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(128);
        thread::spawn(move || {
            while let Ok(Some(bytes)) = crate::wire::read_raw(&mut output, crate::FRAME_MAX) {
                let frame = serde_json::from_slice(&bytes).unwrap();
                if tx.send(frame).is_err() {
                    break;
                }
            }
        });
        Self { child, rx }
    }
    fn until(&self, mut condition: impl FnMut(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let frame = self
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            assert!(!frame.to_string().contains("owned-key-"));
            if condition(&frame) {
                return frame;
            }
        }
    }
    fn send(&mut self, request: &str, command: Value) {
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&json!({"v":3,"request":request,"command":command})).unwrap(),
        );
        crate::wire::write_raw(self.child.stdin.as_mut().unwrap(), &bytes).unwrap();
    }
    fn reply(&self, request: &str) -> Value {
        self.until(|frame| frame["request"] == request)
    }
    fn idle(&mut self) -> Value {
        self.send("settings:view", json!({"op":"view"}));
        let view = self.reply("settings:view")["view"].clone();
        if view["provider_settings"]["busy"] == false {
            return view;
        }
        self.until(|frame| frame["view"]["provider_settings"]["busy"] == false)["view"].clone()
    }
    fn close(&mut self) {
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "native settings shutdown timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn stored(root: &Path) -> Store {
    serde_json::from_slice(&fs::read(root.join("snapshot.json")).unwrap()).unwrap()
}
fn saved(root: &Path) -> (String, Connection, u64) {
    let state = stored(root);
    let slot = state.providers.slot(Protocol::Messages);
    let Selection::Configured { connection } = slot.selection else {
        panic!("configured connection required");
    };
    (state.providers.scope.unwrap(), connection, slot.revision)
}
fn row(view: &Value) -> &Value {
    &view["provider_settings"]["connections"][0]
}
fn replace(endpoint: &str, expected: u64, value: &str) -> Value {
    json!({"op":"provider_save","protocol":"messages","expected_revision":expected,"endpoint":endpoint,"model":crate::https_profile::FIXTURE_MODEL,"key":{"kind":"replace","value":value}})
}
fn consume(profile: &Profile) {
    let stop = AtomicBool::new(false);
    let mut conversation = profile
        .messages_client(&stop)
        .unwrap()
        .start(metldr_messages::Prompt {
            instruction: "inspect",
            title: "Owned settings",
            url: "https://example.org/",
            text: "Owned text",
        })
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut output = String::new();
    assert!(
        runtime
            .block_on(conversation.next(&stop, |chunk| {
                output.push_str(&chunk);
                Ok(())
            }))
            .unwrap()
            .is_none()
    );
    assert_eq!(output, "Settings result");
}

#[test]
#[ignore = "requires isolated owned Secret Service and native executable"]
fn owned_settings_consumer() {
    assert_eq!(
        std::env::var("METLDR_OWNED_VAULT_CHECK").unwrap(),
        "isolated-dummy-only"
    );
    let directory =
        PathBuf::from(std::env::var_os("METLDR_VAULT_CHECK_DIRECTORY").unwrap()).join("managed");
    fs::create_dir(&directory).unwrap();
    let root = directory.join("state");
    let config_path = directory.join("config.json");
    let events = [
        json!({"type":"message_start","message":{"type":"message","id":"msg_settings","model":crate::https_profile::FIXTURE_MODEL,"role":"assistant","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Settings result"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ];
    let body = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    let (endpoint_a, server_a) =
        crate::https_profile::transport_checks::server_key("200 OK", &body, "owned-key-A");
    let (endpoint_b, server_b) =
        crate::https_profile::transport_checks::server_key("200 OK", &body, "owned-key-B");
    let mut config = json!({"state":root,"origin":"http://127.0.0.1:1","extension_origin":EXTENSION,
        "browser_route":{"kind":"https","endpoint":endpoint_a,"model":crate::https_profile::FIXTURE_MODEL,"revision":1,"credential_env":"METLDR_UNUSED_OWNED_KEY","trust_ca":Path::new(env!("CARGO_MANIFEST_DIR")).join("test-tls/ca.pem")}});
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut host = Host::start(&config_path);
    host.until(|frame| frame["kind"] == "ready");
    host.send("save:1", replace(&endpoint_a, 0, "owned-key-A"));
    assert_eq!(host.reply("save:1")["kind"], "view");
    assert_eq!(row(&host.idle())["revision"], 1);
    let (scope, first, revision) = saved(&root);
    let old_profile = first
        .profile(Protocol::Messages, &scope, revision)
        .unwrap()
        .pin()
        .unwrap();
    host.send("save:stale", replace(&endpoint_b, 0, "owned-key-B"));
    assert_eq!(host.reply("save:stale")["kind"], "error");
    host.send("save:2", replace(&endpoint_b, 1, "owned-key-B"));
    assert_eq!(host.reply("save:2")["kind"], "view");
    let view = host.idle();
    assert_eq!(row(&view)["revision"], 2);
    assert_eq!(row(&view)["state"], "ready");
    let (_, second, revision) = saved(&root);
    let new_profile = second
        .profile(Protocol::Messages, &scope, revision)
        .unwrap()
        .pin()
        .unwrap();
    let vault = Vault::new(CredentialId::parse(&scope).unwrap());
    assert!(matches!(
        vault.read(&CredentialId::parse(&first.credential).unwrap()),
        Err(Error::Missing)
    ));
    consume(&old_profile);
    consume(&new_profile);
    server_a.join().unwrap();
    server_b.join().unwrap();
    host.send("save:keep", json!({"op":"provider_save","protocol":"messages","expected_revision":2,"endpoint":endpoint_b,"model":crate::https_profile::FIXTURE_MODEL,"key":{"kind":"keep"}}));
    assert_eq!(host.reply("save:keep")["kind"], "view");
    host.idle();
    let (_, third, revision) = saved(&root);
    assert_eq!(revision, 3);
    assert_ne!(third.credential, second.credential);
    assert!(
        vault
            .read(&CredentialId::parse(&third.credential).unwrap())
            .unwrap()
            .expose()
            == "owned-key-B"
    );
    vault
        .remove(&CredentialId::parse(&third.credential).unwrap())
        .unwrap();
    host.send("refresh:missing", json!({"op":"provider_refresh"}));
    let missing = host.reply("refresh:missing");
    assert_eq!(row(&missing["view"])["state"], "missing");
    assert!(missing["view"]["browser_route"].is_null());
    host.send("save:repair", replace(&endpoint_b, 3, "owned-key-C"));
    // Lose the reply deliberately; shutdown must settle the admitted journaled save.
    host.close();
    assert_eq!(saved(&root).2, 4);
    config["browser_route"]["trust_ca"] = json!("/scratch/no-such-bootstrap-ca");
    config["browser_route"]["credential_env"] = json!("invalid obsolete reference");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let mut host = Host::start(&config_path);
    host.until(|frame| frame["kind"] == "ready");
    assert_eq!(row(&host.idle())["state"], "ready");
    host.send("save:repair", replace(&endpoint_b, 3, "owned-key-C"));
    assert_eq!(
        host.reply("save:repair")["view"]["provider_settings"]["connections"][0]["revision"],
        4
    );
    host.send(
        "remove:1",
        json!({"op":"provider_remove","protocol":"messages","expected_revision":4}),
    );
    assert_eq!(host.reply("remove:1")["kind"], "view");
    assert_eq!(row(&host.idle())["state"], "removed");
    host.close();
    for after_put in [false, true] {
        let mut state = stored(&root);
        let candidate = Connection {
            endpoint: endpoint_b.clone(),
            model: crate::https_profile::FIXTURE_MODEL.into(),
            credential: CredentialId::generate().unwrap().as_str().into(),
            trust_pem: None,
        };
        if after_put {
            vault
                .put(
                    &CredentialId::parse(&candidate.credential).unwrap(),
                    &Key::parse("owned-key-orphan".into()).unwrap(),
                )
                .unwrap();
        }
        state.providers.pending = Some(Pending {
            request: "save:interrupted".into(),
            protocol: Protocol::Messages,
            revision: 6,
            connection: candidate.clone(),
        });
        state.validate().unwrap();
        fs::write(
            root.join("snapshot.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let mut host = Host::start(&config_path);
        host.until(|frame| frame["kind"] == "ready");
        let view = host.idle();
        assert_eq!(row(&view)["state"], "removed");
        assert!(view["browser_route"].is_null());
        assert_eq!(view["provider_settings"]["cleanup_pending"], false);
        host.close();
        assert!(matches!(
            vault.read(&CredentialId::parse(&candidate.credential).unwrap()),
            Err(Error::Missing)
        ));
        assert!(stored(&root).providers.pending.is_none());
    }
    assert!(old_profile.credential().unwrap().to_str().unwrap() == "owned-key-A");
}
