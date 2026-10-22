use super::*;
use metldr_native_credentials::{Error, Key};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::atomic::AtomicBool};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct References {
    scope: String,
    ids: Vec<String>,
}

const LENGTHS: [usize; 4] = [1, 2048, 2049, 4096];

#[test]
#[ignore = "requires an isolated owned Secret Service and explicit phase"]
fn owned_vault_consumer() {
    assert_eq!(
        std::env::var("METLDR_OWNED_VAULT_CHECK").unwrap(),
        "isolated-dummy-only"
    );
    let directory = PathBuf::from(std::env::var_os("METLDR_VAULT_CHECK_DIRECTORY").unwrap());
    let path = directory.join("references.json");
    let phase = std::env::var("METLDR_VAULT_CHECK_PHASE").unwrap();
    if phase == "save" {
        assert!(!path.exists());
        let references = References {
            scope: CredentialId::generate().unwrap().as_str().into(),
            ids: LENGTHS
                .iter()
                .map(|_| CredentialId::generate().unwrap().as_str().into())
                .collect(),
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        use std::io::Write;
        file.write_all(&serde_json::to_vec(&references).unwrap())
            .unwrap();
        file.sync_all().unwrap();
        let vault = Vault::new(CredentialId::parse(&references.scope).unwrap());
        for (id, length) in references.ids.iter().zip(LENGTHS) {
            let id = CredentialId::parse(id).unwrap();
            let key = Key::parse("K".repeat(length)).unwrap();
            vault.put(&id, &key).unwrap();
            vault.put(&id, &key).unwrap();
            assert!(vault.read(&id).unwrap().expose() == key.expose());
            assert!(matches!(
                vault.put(&id, &Key::parse("different".into()).unwrap()),
                Err(Error::Conflict)
            ));
        }
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() < 1024);
    let references: References = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(references.ids.len(), LENGTHS.len());
    let vault = Vault::new(CredentialId::parse(&references.scope).unwrap());
    for (id, length) in references.ids.iter().zip(LENGTHS) {
        let id = CredentialId::parse(id).unwrap();
        match phase.as_str() {
            "read" => {
                assert!(vault.read(&id).unwrap().expose() == "K".repeat(length));
                consume(&references.scope, id.as_str(), length);
            }
            "locked" => {
                assert!(matches!(vault.read(&id), Err(Error::Locked)));
                assert!(matches!(
                    vault.put(&id, &Key::parse("replacement".into()).unwrap()),
                    Err(Error::Locked)
                ));
                assert!(matches!(vault.remove(&id), Err(Error::Locked)));
            }
            "unavailable" => assert!(matches!(vault.read(&id), Err(Error::Unavailable))),
            "remove" => {
                vault.remove(&id).unwrap();
                vault.remove(&id).unwrap();
                assert!(matches!(vault.read(&id), Err(Error::Missing)));
            }
            _ => panic!("unknown owned vault check phase"),
        }
    }
}

fn consume(scope: &str, id: &str, length: usize) {
    let events = [
        json!({"type":"message_start","message":{"type":"message","id":"msg_vault","model":FIXTURE_MODEL,"role":"assistant","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Vault result"}}),
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
    let (endpoint, worker) =
        super::transport_checks::server_key("200 OK", &body, &"K".repeat(length));
    let profile = Config::Keyring {
        endpoint,
        model: FIXTURE_MODEL.into(),
        revision: 1,
        scope: scope.into(),
        credential: id.into(),
        trust_ca: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("test-tls/ca.pem")),
    }
    .bind(Some("127.0.0.1:1".parse().unwrap()))
    .unwrap();
    let public = profile.public().to_string();
    assert!(!public.contains(scope));
    assert!(!public.contains(id));
    let stop = AtomicBool::new(false);
    let client = profile.messages_client(&stop).unwrap();
    let mut conversation = client
        .start(metldr_messages::Prompt {
            instruction: "inspect",
            title: "Owned vault",
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
                if let metldr_conversation::TextEvent::Delta(chunk) = chunk {
                    output.push_str(&chunk);
                }
                Ok(())
            }))
            .unwrap()
            .is_none()
    );
    assert_eq!(output, "Vault result");
    worker.join().unwrap();
}
