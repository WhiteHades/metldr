use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Phase;
use crate::{Prompt, Result, TextEvent, sse, stream};
use metldr_browser_tools::{BROWSER_POLICY, definitions};
use stream::Turn;

pub struct PresentationConversation {
    pub(super) client: Client,
    pub(super) request: Value,
    pub(super) phase: Phase,
}

pub struct PresentationInvocation {
    pub id: String,
    pub presentation: Presentation,
}

pub struct Presentation {
    pub surface_id: String,
    pub table: metldr_conversation::Table,
}

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    request: usize,
    argument: usize,
    sse: usize,
    stream: usize,
    events: usize,
    pause_at_call: bool,
}

pub(super) const BROWSER_BOUNDS: Bounds = Bounds {
    request: 131_072,
    argument: 16_384,
    sse: 131_072,
    stream: 524_288,
    events: 4_096,
    pause_at_call: false,
};
const PRESENTATION_BOUNDS: Bounds = Bounds {
    request: 2_097_152,
    argument: 393_216,
    sse: 1_048_576,
    stream: 8_388_608,
    events: 65_536,
    pause_at_call: true,
};

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
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<()> {
        let turn = self
            .send(
                &self.text_request(system, input),
                stop,
                emit,
                BROWSER_BOUNDS,
            )
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

    pub fn start_presentation(self, system: &str, input: &str) -> Result<PresentationConversation> {
        let row = json!({"type":"array","maxItems":12,"items":{"type":"string","maxLength":2000}});
        let component = json!({"type":"object","properties":{
            "id":{"type":"string","enum":["root"]},"component":{"type":"string","enum":["Table"]},
            "title":{"type":"string","maxLength":300},
            "columns":{"type":"array","minItems":1,"maxItems":12,"items":{"type":"string","maxLength":200}},
            "rows":{"type":"array","maxItems":200,"items":row},
            "source":{"type":"string","maxLength":1000}},
            "required":["id","component","title","columns","rows","source"],"additionalProperties":false});
        let parameters = json!({"type":"object","properties":{
            "catalogId":{"type":"string","enum":["copilotkit://open-generative-ui-tables"]},
            "surfaceId":{"type":"string","minLength":1,"maxLength":256},
            "components":{"type":"array","minItems":1,"maxItems":1,"items":component}},
            "required":["catalogId","surfaceId","components"],"additionalProperties":false});
        let request = json!({"model":self.model,"instructions":system,
            "input":[{"role":"user","content":[{"type":"input_text","text":input}]}],
            "stream":true,"store":false,"include":["reasoning.encrypted_content"],
            "parallel_tool_calls":false,"truncation":"disabled","max_output_tokens":8192,
            "tools":[{"type":"function","name":"render_a2ui","description":"Render one validated table.","strict":true,
                "parameters":parameters}]});
        if serde_json::to_vec(&request)?.len() > PRESENTATION_BOUNDS.request {
            return Err("Responses request exceeds bound".into());
        }
        Ok(PresentationConversation {
            client: self,
            request,
            phase: Phase::Ready,
        })
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
        mut emit: impl FnMut(TextEvent) -> Result<()>,
        bounds: Bounds,
    ) -> Result<Turn> {
        cancelled(stop)?;
        let body = serde_json::to_vec(request)?;
        if body.len() > bounds.request {
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
        let mut sse = sse::Sse::with_bound(bounds.sse);
        let mut state =
            stream::State::with_limits(bounds.argument, bounds.events, bounds.pause_at_call);
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
            if bytes > bounds.stream {
                return Err("Responses stream exceeds bound".into());
            }
            for byte in chunk {
                if let Some((event, data)) = sse.byte(byte)? {
                    cancelled(stop)?;
                    for text_event in state.frame(&event, &data)? {
                        if let TextEvent::Delta(text) = &text_event {
                            rendered.push_str(text);
                        }
                        emit(text_event)?;
                    }
                }
            }
        }
        cancelled(stop)?;
        sse.finish()?;
        let turn = state.finish()?;
        for event in &turn.deferred {
            if let TextEvent::Delta(text) = event {
                rendered.push_str(text);
            }
        }
        if rendered != turn.text {
            return Err("Responses streamed text differs from completed output".into());
        }
        Ok(turn)
    }
}

impl PresentationConversation {
    pub async fn next(
        &mut self,
        stop: &AtomicBool,
        emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<Option<PresentationInvocation>> {
        match self.phase {
            Phase::Ready => {}
            Phase::Pending(_) => return Err("Responses tool result missing".into()),
            Phase::Closed => return Err("Responses conversation is closed".into()),
        }
        self.phase = Phase::Closed;
        let turn = self
            .client
            .send(&self.request, stop, emit, PRESENTATION_BOUNDS)
            .await?;
        let Some(call) = turn.call.as_ref() else {
            return Ok(None);
        };
        let presentation = decode_presentation(&call.name, &call.arguments)?;
        let invocation = PresentationInvocation {
            id: call.id.clone(),
            presentation,
        };
        self.phase = Phase::Pending(turn);
        Ok(Some(invocation))
    }

    pub fn submit_tool_result(
        &mut self,
        result: &Value,
        mut emit: impl FnMut(TextEvent) -> Result<()>,
    ) -> Result<()> {
        if !matches!(self.phase, Phase::Pending(_)) {
            return Err("Responses conversation has no pending tool call".into());
        }
        let output = serde_json::to_string(result)?;
        if output.len() > 393_216 {
            return Err("Responses tool result exceeds bound".into());
        }
        let Phase::Pending(turn) = std::mem::replace(&mut self.phase, Phase::Closed) else {
            unreachable!()
        };
        let call = turn
            .call
            .as_ref()
            .ok_or("Responses turn has no tool call")?;
        let call_id = call.id.clone();
        self.request["input"]
            .as_array_mut()
            .ok_or("invalid Responses request history")?
            .extend(turn.output);
        self.request["input"]
            .as_array_mut()
            .ok_or("invalid Responses request history")?
            .push(json!({"type":"function_call_output","call_id":call_id,"output":output}));
        for event in turn.deferred {
            emit(event)?;
        }
        self.phase = Phase::Ready;
        Ok(())
    }
}

pub(super) fn decode_presentation(name: &str, args: &Value) -> Result<Presentation> {
    if name != "render_a2ui"
        || args.as_object().is_none_or(|v| v.len() != 3)
        || args["catalogId"] != "copilotkit://open-generative-ui-tables"
        || args["components"].as_array().is_none_or(|v| v.len() != 1)
    {
        return Err("invalid render_a2ui arguments".into());
    }
    let surface_id = args["surfaceId"]
        .as_str()
        .ok_or("invalid presentation surface id")?;
    if surface_id.is_empty()
        || surface_id.encode_utf16().count() > 256
        || surface_id.chars().any(char::is_control)
    {
        return Err("invalid presentation surface id".into());
    }
    let component = &args["components"][0];
    if component.as_object().is_none_or(|v| v.len() != 6)
        || component["id"] != "root"
        || component["component"] != "Table"
    {
        return Err("invalid presentation Table component".into());
    }
    let strings = |key: &str| -> Result<Vec<String>> {
        Ok(component[key]
            .as_array()
            .ok_or("invalid table array")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "invalid table string".into())
            })
            .collect::<Result<Vec<_>>>()?)
    };
    let rows = component["rows"]
        .as_array()
        .ok_or("invalid table rows")?
        .iter()
        .map(|row| {
            row.as_array()
                .ok_or("invalid table row")?
                .iter()
                .map(|cell| {
                    cell.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "invalid table cell".into())
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let table = metldr_conversation::Table::new(
        component["title"]
            .as_str()
            .ok_or("invalid table title")?
            .into(),
        strings("columns")?,
        rows,
        component["source"]
            .as_str()
            .ok_or("invalid table source")?
            .into(),
    )
    .map_err(|_| "invalid presentation table")?;
    Ok(Presentation {
        surface_id: surface_id.into(),
        table,
    })
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
