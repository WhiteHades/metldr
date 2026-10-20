use std::cell::RefCell;

use js_sys::{Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::future_to_promise;

use crate::{await_call, call, chrome_api, get};

const STORAGE_KEY: &str = "metldr.nativeProfile";

thread_local! {
    static LOOKUP: RefCell<Option<Promise>> = const { RefCell::new(None) };
}

pub(super) fn is_profile_required(message: &JsValue) -> Result<bool, JsValue> {
    let raw = js_sys::JSON::stringify(message)?
        .as_string()
        .ok_or_else(|| JsValue::from_str("Native frame is not JSON."))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|_| JsValue::from_str("Native frame is invalid JSON."))?;
    if value.get("kind").and_then(serde_json::Value::as_str) != Some("profile_required") {
        return Ok(false);
    }
    let object = value
        .as_object()
        .ok_or_else(|| JsValue::from_str("Invalid profile control frame."))?;
    if object.len() != 2 || value.get("v").and_then(serde_json::Value::as_u64) != Some(3) {
        return Err(JsValue::from_str("Invalid profile control frame."));
    }
    Ok(true)
}

pub(super) fn key() -> Promise {
    LOOKUP.with(|cached| {
        if let Some(promise) = cached.borrow().as_ref() {
            return promise.clone();
        }
        let promise = future_to_promise(async {
            let result = load_or_create().await.map(|key| JsValue::from_str(&key));
            LOOKUP.with(|cached| *cached.borrow_mut() = None);
            result
        });
        *cached.borrow_mut() = Some(promise.clone());
        promise
    })
}

async fn load_or_create() -> Result<String, JsValue> {
    let storage = get(&chrome_api("storage")?, "local")?;
    let stored = await_call(&storage, "get", &[JsValue::from_str(STORAGE_KEY)]).await?;
    let value = get(&stored, STORAGE_KEY)?;
    let key = if value.is_undefined() {
        let key = random_key()?;
        let patch = Object::new();
        Reflect::set(
            &patch,
            &JsValue::from_str(STORAGE_KEY),
            &JsValue::from_str(&key),
        )?;
        await_call(&storage, "set", &[patch.into()]).await?;
        key
    } else {
        value
            .as_string()
            .filter(|key| valid_key(key))
            .ok_or_else(|| JsValue::from_str("Stored native profile is malformed."))?
    };
    let verified = await_call(&storage, "get", &[JsValue::from_str(STORAGE_KEY)]).await?;
    if get(&verified, STORAGE_KEY)?.as_string().as_deref() != Some(&key) {
        return Err(JsValue::from_str("Native profile storage readback failed."));
    }
    Ok(key)
}

fn random_key() -> Result<String, JsValue> {
    let crypto = get(&js_sys::global(), "crypto")?;
    let bytes = Uint8Array::new_with_length(32);
    call(&crypto, "getRandomValues", &[bytes.clone().into()])?;
    Ok(bytes
        .to_vec()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn valid_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
