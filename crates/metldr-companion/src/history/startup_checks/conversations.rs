use super::{Directory, Running, fixture};
use crate::Result;
use serde_json::{Value, json};
use std::fs;

fn conversations(
    host: &mut Running,
    request: &str,
    scope: &Value,
    generation: &Value,
) -> Result<Value> {
    host.send(
        request,
        json!({"op":"history_conversations","scope":scope,"generation":generation,"cursor":null}),
    )?;
    let frame = host.reply(request)?;
    assert_eq!(frame["kind"], "history_conversations");
    Ok(frame["page"].clone())
}

fn runs(host: &mut Running, request: &str, scope: &Value, generation: &Value) -> Result<Value> {
    host.send(
        request,
        json!({"op":"history_runs","scope":scope,"generation":generation,"cursor":null}),
    )?;
    Ok(host.reply(request)?)
}

fn change(host: &mut Running, request: &str, scope: &Value, change: Value) -> Result<Value> {
    host.send(
        request,
        json!({"op":"conversation_change","scope":scope,"change":change}),
    )?;
    Ok(host.reply(request)?)
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY with the current companion"]
fn native_conversation_change_replay_and_stale_history_survive_restart() -> Result<()> {
    let directory = Directory::new()?;
    let mut legacy = fixture::legacy();
    legacy.providers = Default::default();
    legacy.agent_settings = Default::default();
    legacy.validate()?;
    let original = serde_json::to_vec(&legacy)?;
    fs::write(directory.0.join("snapshot.json"), &original)?;
    let mut host = Running::start(&directory.0)?;
    let ready = host.ready()?;
    let marker = fs::read(directory.0.join("snapshot.json"))?;
    let backup = fs::read(directory.0.join("snapshot.v8.json"))?;
    assert_eq!(serde_json::from_slice::<Value>(&marker)?["schema"], 9);
    assert_eq!(backup, original);
    let sequence = ready["view"]["sequence"].clone();
    host.send("current-a", json!({"op":"history_current"}))?;
    let current_a = host.reply("current-a")?;
    let scope_a = current_a["scope"].clone();
    let generation_a = current_a["generation"].clone();
    let list_a = conversations(&mut host, "list-a", &scope_a, &generation_a)?;
    assert_eq!(list_a["conversations"].as_array().unwrap().len(), 1);
    let a = list_a["conversations"][0]["id"].clone();
    let legacy_runs = runs(&mut host, "runs-a", &scope_a, &generation_a)?;
    assert_eq!(legacy_runs["kind"], "history_runs");
    assert_eq!(legacy_runs["page"]["runs"].as_array().unwrap().len(), 11);

    host.send(
        "pin-a",
        json!({"op":"history_pin","scope":scope_a,"generation":generation_a,"kind":"run","document":format!("{}:01", current_a["scope"]["store"].as_str().unwrap()),"revision":"1"}),
    )?;
    let pinned = host.reply("pin-a")?;
    assert_eq!(pinned["kind"], "history_pinned");
    assert!(pinned["identity"].is_object());
    let pin_a = pinned["identity"].clone();
    let create = json!({"kind":"create"});
    let created = change(&mut host, "create-b", &scope_a, create.clone())?;
    assert_eq!(created["kind"], "conversation_changed");
    assert_eq!(created["original"], created["current"]);
    let scope_b = created["current"].clone();
    let generation_b = created["generation"].clone();
    assert_ne!(scope_b["conversation"], a);
    assert_eq!(scope_b["epoch"], "2");
    assert_eq!(
        created["sequence"],
        (sequence.as_u64().unwrap() + 1).to_string()
    );
    assert_eq!(
        runs(&mut host, "runs-b", &scope_b, &generation_b)?["page"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let list_b = conversations(&mut host, "list-b", &scope_b, &generation_b)?;
    assert_eq!(list_b["conversations"].as_array().unwrap().len(), 2);
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);
    assert_eq!(fs::read(directory.0.join("snapshot.v8.json"))?, backup);

    host.send(
        "stale-a-b",
        json!({"op":"history_runs","scope":scope_a,"generation":generation_a,"cursor":null}),
    )?;
    assert_eq!(host.reply("stale-a-b")?["kind"], "history_stale");
    host.send(
        "stale-pin-b",
        json!({"op":"history_page","identity":pin_a,"offset":0}),
    )?;
    assert_eq!(host.reply("stale-pin-b")?["kind"], "history_stale");

    let select_a = json!({"kind":"select","conversation":a});
    let selected_a = change(&mut host, "select-a", &scope_b, select_a)?;
    assert_eq!(selected_a["kind"], "conversation_changed");
    let scope_a2 = selected_a["current"].clone();
    let generation_a2 = selected_a["generation"].clone();
    assert_eq!(scope_a2["conversation"], a);
    assert_eq!(scope_a2["epoch"], "3");
    assert_eq!(
        runs(&mut host, "runs-a-return", &scope_a2, &generation_a2)?["page"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        11
    );
    for (request, scope, generation) in [
        ("stale-a-return", &scope_a, &generation_a),
        ("stale-b-return", &scope_b, &generation_b),
    ] {
        host.send(request, json!({"op":"history_conversations","scope":scope,"generation":generation,"cursor":null}))?;
        assert_eq!(host.reply(request)?["kind"], "history_stale");
    }
    host.send(
        "stale-pin-return",
        json!({"op":"history_page","identity":pin_a,"offset":0}),
    )?;
    assert_eq!(host.reply("stale-pin-return")?["kind"], "history_stale");

    let retry = change(&mut host, "create-b", &scope_a, create.clone())?;
    assert_eq!(retry["kind"], "conversation_changed");
    assert_eq!(retry["original"], scope_b);
    assert_eq!(retry["current"], scope_a2);
    assert_eq!(retry["sequence"], selected_a["sequence"]);
    let mut changed = create;
    changed["kind"] = json!("select");
    changed["conversation"] = a;
    let rejected = change(&mut host, "create-b", &scope_a, changed)?;
    assert_eq!(rejected["kind"], "error");
    let final_list = conversations(&mut host, "final-list", &scope_a2, &generation_a2)?;
    assert_eq!(final_list["conversations"].as_array().unwrap().len(), 2);
    assert!(host.finish()?.success());

    let mut reopened = Running::start(&directory.0)?;
    reopened.ready()?;
    let replay = change(
        &mut reopened,
        "create-b",
        &scope_a,
        json!({"kind":"create"}),
    )?;
    assert_eq!(replay["kind"], "conversation_changed");
    assert_eq!(replay["original"], scope_b);
    assert_eq!(replay["current"], scope_a2);
    assert_eq!(replay["sequence"], selected_a["sequence"]);
    let list = conversations(
        &mut reopened,
        "restart-list",
        &scope_a2,
        &replay["generation"],
    )?;
    assert_eq!(list["conversations"].as_array().unwrap().len(), 2);
    assert!(reopened.finish()?.success());
    assert_eq!(fs::read(directory.0.join("snapshot.json"))?, marker);
    assert_eq!(fs::read(directory.0.join("snapshot.v8.json"))?, backup);
    Ok(())
}

#[test]
#[ignore = "requires METLDR_STARTUP_BINARY with the current companion"]
fn delayed_capture_cannot_cross_selected_conversations() -> Result<()> {
    let directory = Directory::new()?;
    super::empty_legacy(&directory.0, Some(8))?;
    let mut host = Running::start(&directory.0)?;
    let ready = host.ready()?;
    assert!(ready["view"]["source"].is_null());
    host.send("capture-origin", json!({"op":"history_current"}))?;
    let current = host.reply("capture-origin")?;
    let scope_a = current["scope"].clone();
    let action = json!({
        "op":"capture", "expected_source":null, "page":{
            "tab":1,"document":"queued-a-document",
            "url":format!("{}/capture",super::ORIGIN),
            "title":"Only conversation A",
            "text":"This action was prepared before B existed."
        }
    });
    let queued_capture = json!({"op":"conversation","scope":scope_a,"action":action});
    assert!(crate::wire::decode_request(&serde_json::to_vec(&json!({
        "v":crate::WIRE_VERSION,"request":"retired-capture",
        "command":{"op":"capture","expected_store":scope_a["store"],"expected_source":null,"page":action["page"]}
    }))?).is_err());
    let created = change(
        &mut host,
        "capture-target",
        &scope_a,
        json!({"kind":"create"}),
    )?;
    assert_eq!(created["kind"], "conversation_changed");
    assert_ne!(created["current"]["conversation"], scope_a["conversation"]);
    host.send("queued-a", queued_capture.clone())?;
    let reply = host.reply("queued-a")?;
    eprintln!(
        "delayed capture result: {}",
        json!({
            "kind":reply["kind"],
            "title":reply["view"]["source"]["title"],
            "target":created["current"]
        })
    );
    assert_eq!(
        reply["kind"], "error",
        "a delayed action must not mutate a newly selected conversation"
    );
    host.send("untouched-b", json!({"op":"view"}))?;
    let untouched = host.reply("untouched-b")?;
    assert_eq!(untouched["view"]["selection"], created["current"]);
    assert!(untouched["view"]["source"].is_null());
    let selected = change(
        &mut host,
        "return-a",
        &created["current"],
        json!({"kind":"select","conversation":scope_a["conversation"]}),
    )?;
    host.send("queued-a-return", queued_capture)?;
    assert_eq!(host.reply("queued-a-return")?["kind"], "error");
    let capture = json!({"op":"conversation","scope":selected["current"],"action":action});
    host.send("fresh-a", capture.clone())?;
    let captured = host.reply("fresh-a")?;
    assert_eq!(captured["kind"], "view");
    assert_eq!(captured["view"]["selection"], selected["current"]);
    assert_eq!(captured["view"]["source"]["title"], "Only conversation A");
    assert!(captured["view"]["runs"].as_array().unwrap().is_empty());
    host.send("repeat-a", capture.clone())?;
    let repeated = host.reply("repeat-a")?;
    assert_eq!(
        repeated["view"]["source"]["id"],
        captured["view"]["source"]["id"]
    );
    assert_eq!(repeated["view"]["sequence"], captured["view"]["sequence"]);
    let mut stale_source = capture;
    stale_source["action"]["page"]["title"] = json!("Changed source");
    host.send("stale-source", stale_source)?;
    assert_eq!(host.reply("stale-source")?["kind"], "error");
    assert!(host.finish()?.success());
    let reopened = Running::start(&directory.0)?;
    let saved = reopened.ready()?;
    assert_eq!(saved["view"]["selection"], selected["current"]);
    assert_eq!(
        saved["view"]["source"]["id"],
        captured["view"]["source"]["id"]
    );
    assert!(saved["view"]["runs"].as_array().unwrap().is_empty());
    assert!(reopened.finish()?.success());
    Ok(())
}
