use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Prompt, Result, sse, stream};
use metldr_browser_tools::{BROWSER_POLICY, definitions};
use stream::Turn;

pub struct Client {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    authorization: reqwest::header::HeaderValue,
}

impl Client {
    pub fn new(
        endpoint: &str,
        model: &str,
        bearer_token: &str,
        trust_ca_pem: Option<&[u8]>,
    ) -> Result<Self> {
        let url = reqwest::Url::parse(endpoint).map_err(|_| "invalid Responses endpoint")?;
        if endpoint.len() > 1024
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.port() == Some(0)
        {
            return Err("invalid Responses endpoint".into());
        }
        if model.is_empty() || model.len() > 128 || !model.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("invalid Responses model".into());
        }
        if bearer_token.is_empty()
            || bearer_token.len() > 4096
            || !bearer_token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err("invalid Responses credential".into());
        }
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {bearer_token}"))
                .map_err(|_| "invalid Responses credential")?;
        authorization.set_sensitive(true);
        let mut client = reqwest::Client::builder()
            .tls_backend_rustls()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(std::time::Duration::from_secs(45));
        if let Some(pem) = trust_ca_pem {
            if pem.len() > 16_384 {
                return Err("Responses trust certificate exceeds bound".into());
            }
            let certificates = reqwest::Certificate::from_pem_bundle(pem)
                .map_err(|_| "invalid Responses trust certificate")?;
            if certificates.is_empty() {
                return Err("empty Responses trust certificate".into());
            }
            client = client.tls_certs_only(certificates);
        }
        Ok(Self {
            client: client.build().map_err(|_| "Responses client unavailable")?,
            endpoint: url.to_string(),
            model: model.into(),
            authorization,
        })
    }

    /// Streams one text response without offering tools. Partial text can precede an error.
    pub async fn text(
        self,
        system: &str,
        input: &str,
        stop: &AtomicBool,
        emit: impl FnMut(String) -> Result<()>,
    ) -> Result<()> {
        let turn = self
            .send(&self.text_request(system, input), stop, emit)
            .await?;
        if turn.call.is_some() {
            return Err("Responses text request returned a tool call".into());
        }
        Ok(())
    }

    fn text_request(&self, system: &str, input: &str) -> Value {
        json!({"model":self.model,"instructions":system,
            "input":[{"role":"user","content":[{"type":"input_text","text":input}]}],
            "stream":true,"store":false,"truncation":"disabled","max_output_tokens":2048})
    }

    pub(super) fn request(&self, prompt: Prompt<'_>) -> Value {
        let context = json!({"instruction":prompt.instruction,"source":{
            "title":prompt.title,"url":prompt.url,"text":prompt.text}});
        let tools: Vec<_> = definitions().into_iter().map(|definition| {
            let mut parameters = definition.input_schema;
            if parameters.get("required").is_none() { parameters["required"] = json!([]); }
            json!({"type":"function","name":definition.name,"description":definition.description,
                "parameters":parameters,"strict":true})
        }).collect();
        json!({"model":self.model,"instructions":BROWSER_POLICY,
            "input":[{"role":"user","content":[{"type":"input_text","text":context.to_string()}]}],
            "stream":true,"store":false,"include":["reasoning.encrypted_content"],
            "parallel_tool_calls":false,"truncation":"disabled","max_output_tokens":2048,
            "tools":tools})
    }

    pub(super) async fn send(
        &self,
        request: &Value,
        stop: &AtomicBool,
        mut emit: impl FnMut(String) -> Result<()>,
    ) -> Result<Turn> {
        cancelled(stop)?;
        let body = serde_json::to_vec(request)?;
        if body.len() > 131_072 {
            return Err("Responses request exceeds bound".into());
        }
        let sending = self
            .client
            .post(&self.endpoint)
            .header("authorization", self.authorization.clone())
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .body(body)
            .send();
        let mut response = tokio::select! {
            biased;
            _ = wait_for_stop(stop) => return Err("Responses request cancelled".into()),
            response = sending => response.map_err(|_| "Responses transport failed")?,
        };
        if response.status() != reqwest::StatusCode::OK {
            return Err("Responses endpoint returned an error".into());
        }
        if response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_none_or(|v| !v.trim().eq_ignore_ascii_case("text/event-stream"))
        {
            return Err("Responses endpoint did not return an event stream".into());
        }
        let mut sse = sse::Sse::default();
        let mut state = stream::State::default();
        let mut bytes = 0usize;
        let mut rendered = String::new();
        loop {
            let chunk = tokio::select! {
                biased;
                _ = wait_for_stop(stop) => return Err("Responses request cancelled".into()),
                chunk = response.chunk() => chunk.map_err(|_| "Responses stream transport failed")?,
            };
            let Some(chunk) = chunk else { break };
            bytes = bytes.saturating_add(chunk.len());
            if bytes > 524_288 {
                return Err("Responses stream exceeds bound".into());
            }
            for byte in chunk {
                if let Some((event, data)) = sse.byte(byte)? {
                    cancelled(stop)?;
                    if let Some(text) = state.frame(&event, &data)? {
                        rendered.push_str(&text);
                        emit(text)?;
                    }
                }
            }
        }
        cancelled(stop)?;
        sse.finish()?;
        let turn = state.finish()?;
        if rendered != turn.text {
            return Err("Responses streamed text differs from completed output".into());
        }
        Ok(turn)
    }
}

fn cancelled(stop: &AtomicBool) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        Err("Responses request cancelled".into())
    } else {
        Ok(())
    }
}

async fn wait_for_stop(stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}
