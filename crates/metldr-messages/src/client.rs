use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Prompt, Result, TextEvent, sse, stream};
use metldr_browser_tools::{BROWSER_POLICY, definitions};
use stream::Turn;

pub struct Client {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    key: reqwest::header::HeaderValue,
}

impl Client {
    pub fn new(
        endpoint: &str,
        model: &str,
        api_key: &str,
        trust_ca_pem: Option<&[u8]>,
    ) -> Result<Self> {
        let url = reqwest::Url::parse(endpoint).map_err(|_| "invalid Messages endpoint")?;
        if endpoint.len() > 1024
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port() == Some(0)
        {
            return Err("invalid Messages endpoint".into());
        }
        if model.is_empty() || model.len() > 128 || !model.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("invalid Messages model".into());
        }
        if api_key.is_empty()
            || api_key.len() > 4096
            || !api_key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err("invalid Messages credential".into());
        }
        let mut key = reqwest::header::HeaderValue::from_str(api_key)
            .map_err(|_| "invalid Messages credential")?;
        key.set_sensitive(true);
        let mut client = reqwest::Client::builder()
            .tls_backend_rustls()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(std::time::Duration::from_secs(45));
        if let Some(pem) = trust_ca_pem {
            if pem.len() > 16_384 {
                return Err("Messages trust certificate exceeds bound".into());
            }
            let certificates = reqwest::Certificate::from_pem_bundle(pem)
                .map_err(|_| "invalid Messages trust certificate")?;
            if certificates.is_empty() {
                return Err("empty Messages trust certificate".into());
            }
            client = client.tls_certs_only(certificates);
        }
        Ok(Self {
            client: client.build().map_err(|_| "Messages client unavailable")?,
            endpoint: url.to_string(),
            model: model.into(),
            key,
        })
    }

    /// Streams one text response without offering tools. Partial text can precede an error.
    pub async fn text(
        self,
        system: &str,
        input: &str,
        stop: &AtomicBool,
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<()> {
        let turn = self
            .send(&self.text_request(system, input), stop, emit)
            .await?;
        if turn.call.is_some() {
            return Err("Messages text request returned a tool call".into());
        }
        Ok(())
    }

    fn text_request(&self, system: &str, input: &str) -> Value {
        json!({"model":self.model,"system":system,"max_tokens":2048,"stream":true,
            "messages":[{"role":"user","content":[{"type":"text","text":input}]}]})
    }

    pub(super) fn request(&self, prompt: Prompt<'_>) -> Value {
        let context = json!({"instruction":prompt.instruction,"source":{
            "title":prompt.title,"url":prompt.url,"text":prompt.text}});
        let tools: Vec<_> = definitions()
            .into_iter()
            .map(|definition| {
                json!({"name":definition.name,"description":definition.description,
                "input_schema":definition.input_schema})
            })
            .collect();
        json!({"model":self.model,"system":BROWSER_POLICY,"max_tokens":2048,
            "stream":true,"tools":tools,
            "messages":[{"role":"user","content":[{"type":"text","text":context.to_string()}]}]})
    }

    pub(super) async fn send(
        &self,
        request: &Value,
        stop: &AtomicBool,
        mut emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<Turn> {
        cancelled(stop)?;
        let body = serde_json::to_vec(request)?;
        if body.len() > 131_072 {
            return Err("Messages request exceeds bound".into());
        }
        let sending = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", self.key.clone())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .body(body)
            .send();
        let mut response = tokio::select! {
            biased;
            _ = wait_for_stop(stop) => return Err("Messages request cancelled".into()),
            response = sending => response.map_err(|_| "Messages transport failed")?,
        };
        if response.status() != reqwest::StatusCode::OK {
            return Err("Messages endpoint returned an error".into());
        }
        if response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_none_or(|v| !v.trim().eq_ignore_ascii_case("text/event-stream"))
        {
            return Err("Messages endpoint did not return an event stream".into());
        }
        let mut sse = sse::Sse::default();
        let mut state = stream::State::new(&self.model);
        let mut bytes = 0usize;
        let mut rendered = String::new();
        loop {
            let chunk = tokio::select! {
                biased;
                _ = wait_for_stop(stop) => return Err("Messages request cancelled".into()),
                chunk = response.chunk() => chunk.map_err(|_| "Messages stream transport failed")?,
            };
            let Some(chunk) = chunk else { break };
            bytes = bytes.saturating_add(chunk.len());
            if bytes > 524_288 {
                return Err("Messages stream exceeds bound".into());
            }
            for byte in chunk {
                if let Some((event, data)) = sse.byte(byte)? {
                    cancelled(stop)?;
                    if let Some(event) = state.frame(&event, &data)? {
                        if let TextEvent::Delta(text) = &event {
                            rendered.push_str(text);
                        }
                        emit(event)?;
                    }
                }
            }
        }
        cancelled(stop)?;
        sse.finish()?;
        let turn = state.finish()?;
        if rendered != turn.text {
            return Err("Messages streamed text differs from completed output".into());
        }
        if !turn.text.is_empty() {
            emit(TextEvent::Final(turn.text.clone()))?;
        }
        Ok(turn)
    }
}

fn cancelled(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        Err("Messages request cancelled".into())
    } else {
        Ok(())
    }
}

async fn wait_for_stop(stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
