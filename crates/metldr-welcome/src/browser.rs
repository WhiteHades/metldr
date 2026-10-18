use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;

const SETTINGS_KEYS: [&str; 6] = [
    "theme",
    "preferredProvider",
    "selectedModel",
    "onboardingComplete",
    "chromeAIDownloadComplete",
    "chromeAIStatus",
];
const OLLAMA_TAGS_URL: &str = "http://127.0.0.1:11434/api/tags";
const MAX_TAGS_BODY: usize = 256 * 1024;

pub fn runtime_id() -> Result<String, JsValue> {
    let chrome = get(&js_sys::global(), "chrome")?;
    get(&get(&chrome, "runtime")?, "id")?
        .as_string()
        .ok_or_else(|| JsValue::from_str("Extension ID unavailable"))
}

pub fn start_clipboard_write(text: &str) -> Result<Promise, JsValue> {
    let navigator = get(&js_sys::global(), "navigator")?;
    let clipboard = get(&navigator, "clipboard")?;
    call(&clipboard, "writeText", &[JsValue::from_str(text)])?.dyn_into::<Promise>()
}

pub fn get(object: &JsValue, key: &str) -> Result<JsValue, JsValue> {
    Reflect::get(object, &JsValue::from_str(key))
}

pub fn call(object: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function = get(object, method)?.dyn_into::<Function>()?;
    let args = Array::from_iter(args.iter());
    function.apply(object, &args)
}

pub async fn await_call(
    object: &JsValue,
    method: &str,
    args: &[JsValue],
) -> Result<JsValue, JsValue> {
    JsFuture::from(Promise::resolve(&call(object, method, args)?)).await
}

pub async fn load_settings() -> Result<JsValue, JsValue> {
    let storage = get(&get(&js_sys::global(), "chrome")?, "storage")?;
    let local = get(&storage, "local")?;
    let keys = Array::new();
    for key in SETTINGS_KEYS {
        keys.push(&JsValue::from_str(key));
    }
    await_call(&local, "get", &[keys.into()]).await
}

pub async fn save_settings(value: &JsValue) -> Result<(), JsValue> {
    let settings = Object::new();
    for key in SETTINGS_KEYS {
        let field = get(value, key)?;
        if !field.is_undefined() {
            let valid = match key {
                "theme" => matches!(
                    field.as_string().as_deref(),
                    Some("catppuccin" | "gruvbox" | "cyberpunk" | "light")
                ),
                "preferredProvider" => {
                    matches!(field.as_string().as_deref(), Some("chrome-ai" | "ollama"))
                }
                "selectedModel" => field
                    .as_string()
                    .is_some_and(|name| !name.is_empty() && name.len() <= 512),
                "onboardingComplete" => field.as_bool().is_some(),
                "chromeAIDownloadComplete" => field
                    .as_f64()
                    .is_some_and(|time| time.is_finite() && time > 0.0),
                "chromeAIStatus" => field.as_string().as_deref() == Some("available"),
                _ => false,
            };
            if !valid {
                return Err(JsValue::from_str("Invalid settings value"));
            }
            Reflect::set(&settings, &JsValue::from_str(key), &field)?;
        }
    }
    let storage = get(&get(&js_sys::global(), "chrome")?, "storage")?;
    let local = get(&storage, "local")?;
    await_call(&local, "set", &[settings.into()]).await?;
    Ok(())
}

pub fn object(fields: &[(&str, JsValue)]) -> Result<JsValue, JsValue> {
    let result = Object::new();
    for (key, value) in fields {
        Reflect::set(&result, &JsValue::from_str(key), value)?;
    }
    Ok(result.into())
}

pub fn summarizer() -> Option<JsValue> {
    get(&js_sys::global(), "Summarizer")
        .ok()
        .filter(|value| !value.is_null() && !value.is_undefined())
}

struct FetchTimeout {
    controller: JsValue,
    timer: JsValue,
    callback: Closure<dyn FnMut()>,
}

impl Drop for FetchTimeout {
    fn drop(&mut self) {
        let global = js_sys::global();
        if let Ok(clear_timeout) = get(&global, "clearTimeout") {
            if let Ok(clear_timeout) = clear_timeout.dyn_into::<Function>() {
                let _ = clear_timeout.call1(&global, &self.timer);
            }
        }
        let _ = call(&self.controller, "abort", &[]);
        let _ = &self.callback;
    }
}

pub async fn check_ollama() -> Result<Vec<String>, JsValue> {
    let global = js_sys::global();
    let constructor = get(&global, "AbortController")?;
    let controller = Reflect::construct(&constructor.dyn_into::<Function>()?, &Array::new())?;
    let abort_controller = controller.clone();
    let callback = Closure::<dyn FnMut()>::new(move || {
        let _ = call(&abort_controller, "abort", &[]);
    });
    let set_timeout = get(&global, "setTimeout")?.dyn_into::<Function>()?;
    let timer = set_timeout.call2(&global, callback.as_ref(), &JsValue::from_f64(3_000.0))?;
    let _timeout = FetchTimeout {
        controller: controller.clone(),
        timer,
        callback,
    };

    let options = Object::new();
    Reflect::set(
        &options,
        &JsValue::from_str("signal"),
        &get(&controller, "signal")?,
    )?;
    let response = JsFuture::from(Promise::resolve(&call(
        &global,
        "fetch",
        &[JsValue::from_str(OLLAMA_TAGS_URL), options.into()],
    )?))
    .await?;
    if !get(&response, "ok")?.as_bool().unwrap_or(false) {
        return Err(JsValue::from_str("Ollama tags request failed"));
    }
    let body = read_limited_json(&response).await?;
    let models = get(&body, "models")?
        .dyn_into::<Array>()
        .map_err(|_| JsValue::from_str("Ollama response has no models array"))?;
    if models.length() > 512 {
        return Err(JsValue::from_str("Ollama returned too many models"));
    }
    models
        .iter()
        .map(|model| {
            let name = get(&model, "name")?
                .as_string()
                .ok_or_else(|| JsValue::from_str("Ollama model name is invalid"))?;
            if name.is_empty() || name.len() > 512 {
                return Err(JsValue::from_str("Ollama model name is invalid"));
            }
            Ok(name)
        })
        .collect()
}

async fn read_limited_json(response: &JsValue) -> Result<JsValue, JsValue> {
    let body = get(response, "body")?;
    if body.is_null() || body.is_undefined() {
        return Err(JsValue::from_str("Ollama response has no body"));
    }
    let reader = call(&body, "getReader", &[])?;
    let result = read_stream(&reader).await;
    let _ = await_call(&reader, "cancel", &[]).await;
    let _ = call(&reader, "releaseLock", &[]);
    result
}

async fn read_stream(reader: &JsValue) -> Result<JsValue, JsValue> {
    let mut bytes = Vec::new();
    loop {
        let chunk = await_call(reader, "read", &[]).await?;
        if get(&chunk, "done")?.as_bool().unwrap_or(false) {
            break;
        }
        let value = get(&chunk, "value")?;
        let array = value.dyn_into::<Uint8Array>()?;
        let length = array.length() as usize;
        if bytes.len().saturating_add(length) > MAX_TAGS_BODY {
            return Err(JsValue::from_str("Ollama response is too large"));
        }
        let start = bytes.len();
        bytes.resize(start + length, 0);
        array.copy_to(&mut bytes[start..]);
    }
    let text =
        String::from_utf8(bytes).map_err(|_| JsValue::from_str("Ollama returned invalid UTF8"))?;
    js_sys::JSON::parse(&text)
}
