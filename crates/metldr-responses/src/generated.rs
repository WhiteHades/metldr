use metldr_conversation::{AnswerHeight, CompleteSource, GeneratedAnswer, GeneratedUpdate};
use serde::Deserialize;
use serde_json::Value;

pub(crate) struct GeneratedProgress {
    pub surface_id: String,
    pub content: GeneratedAnswer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field {
    SurfaceId,
    InitialHeight,
    Css,
    Html,
    JsFunctions,
    JsExpressions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObjectState {
    Start,
    KeyOrEnd,
    Colon,
    Value(Field),
    CommaOrEnd,
    Done,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArrayState {
    ValueOrEnd,
    Value,
    CommaOrEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StringUse {
    Key,
    Value(Field),
    Element(Field),
}

pub(crate) struct GeneratedArguments {
    limit: usize,
    cursor: usize,
    scanned: usize,
    pending_string: Option<(usize, usize, bool, StringUse)>,
    object: ObjectState,
    pending_field: Option<Field>,
    array: Option<(Field, ArrayState)>,
    seen: u8,
    surface_id: Option<String>,
    emitted_surface_id: Option<String>,
    content: GeneratedAnswer,
    poisoned: bool,
    emitted: Option<GeneratedAnswer>,
}

impl GeneratedArguments {
    pub(crate) fn new(raw_byte_limit: usize) -> Self {
        Self {
            limit: raw_byte_limit,
            cursor: 0,
            scanned: 0,
            pending_string: None,
            object: ObjectState::Start,
            pending_field: None,
            array: None,
            seen: 0,
            surface_id: None,
            emitted_surface_id: None,
            content: GeneratedAnswer::default(),
            poisoned: false,
            emitted: None,
        }
    }

    pub(crate) fn push(&mut self, prefix: &str) -> Result<Option<GeneratedProgress>, String> {
        if self.poisoned {
            return Err("generated argument decoder is poisoned".into());
        }
        let result = self.scan(prefix);
        match result {
            Ok(()) => {
                if self.surface_id.is_some() && self.emitted.as_ref() != Some(&self.content) {
                    self.emitted = Some(self.content.clone());
                    self.emitted_surface_id = self.surface_id.clone();
                    Ok(Some(GeneratedProgress {
                        surface_id: self.surface_id.clone().unwrap(),
                        content: self.content.clone(),
                    }))
                } else {
                    Ok(None)
                }
            }
            Err(error) => {
                self.poisoned = true;
                Err(error)
            }
        }
    }

    pub(crate) fn finish(mut self, full: &str) -> Result<(String, CompleteSource), String> {
        if self.poisoned {
            return Err("generated argument decoder is poisoned".into());
        }
        self.scan(full)?;
        if self.scanned != full.len() || self.object != ObjectState::Done || self.array.is_some() {
            return Err("generated arguments are incomplete".into());
        }
        let raw: RawArguments =
            serde_json::from_str(full).map_err(|_| "invalid generated arguments")?;
        if self
            .emitted_surface_id
            .as_ref()
            .is_some_and(|surface_id| surface_id != &raw.surface_id)
        {
            return Err("generated surface id differs from emitted identity".into());
        }
        let expected = from_raw(&raw)?;
        if expected != self.content {
            return Err("generated arguments differ from incremental content".into());
        }
        Ok((raw.surface_id, expected.finish()?))
    }

    fn scan(&mut self, prefix: &str) -> Result<(), String> {
        if prefix.len() > self.limit {
            return Err("generated arguments exceed byte limit".into());
        }
        if prefix.len() < self.scanned {
            return Err("generated arguments are not append-only".into());
        }
        while self.cursor < prefix.len() {
            if self.pending_string.is_some() {
                if !self.scan_string(prefix)? {
                    break;
                }
                continue;
            }
            self.whitespace(prefix);
            if self.cursor == prefix.len() {
                break;
            }
            if let Some((field, state)) = self.array {
                match state {
                    ArrayState::ValueOrEnd => match prefix.as_bytes()[self.cursor] {
                        b']' => {
                            self.cursor += 1;
                            self.array = None;
                            self.apply(match field {
                                Field::Html => GeneratedUpdate::HtmlComplete,
                                Field::JsExpressions => GeneratedUpdate::ExpressionsComplete,
                                _ => return Err("invalid generated array field".into()),
                            })?;
                            self.object = ObjectState::CommaOrEnd;
                        }
                        b'"' => {
                            self.start_string(prefix, StringUse::Element(field));
                        }
                        _ => return Err("invalid generated array value".into()),
                    },
                    ArrayState::Value => match prefix.as_bytes()[self.cursor] {
                        b'"' => self.start_string(prefix, StringUse::Element(field)),
                        _ => return Err("invalid generated array value".into()),
                    },
                    ArrayState::CommaOrEnd => match prefix.as_bytes()[self.cursor] {
                        b',' => {
                            self.cursor += 1;
                            self.array = Some((field, ArrayState::Value));
                        }
                        b']' => {
                            self.cursor += 1;
                            self.array = None;
                            self.apply(match field {
                                Field::Html => GeneratedUpdate::HtmlComplete,
                                Field::JsExpressions => GeneratedUpdate::ExpressionsComplete,
                                _ => return Err("invalid generated array field".into()),
                            })?;
                            self.object = ObjectState::CommaOrEnd;
                        }
                        _ => return Err("invalid generated array separator".into()),
                    },
                }
                continue;
            }
            match self.object {
                ObjectState::Start => {
                    if prefix.as_bytes()[self.cursor] != b'{' {
                        return Err("generated arguments must start with an object".into());
                    }
                    self.cursor += 1;
                    self.object = ObjectState::KeyOrEnd;
                }
                ObjectState::KeyOrEnd => match prefix.as_bytes()[self.cursor] {
                    b'}' if self.seen == 0 => {
                        self.cursor += 1;
                        self.object = ObjectState::Done;
                    }
                    b'"' => self.start_string(prefix, StringUse::Key),
                    _ => return Err("invalid generated object key".into()),
                },
                ObjectState::Colon => {
                    if prefix.as_bytes()[self.cursor] != b':' {
                        return Err("missing generated field colon".into());
                    }
                    self.cursor += 1;
                    self.object = ObjectState::Value(self.current_field()?);
                }
                ObjectState::Value(field) => match prefix.as_bytes()[self.cursor] {
                    b'"' if matches!(field, Field::SurfaceId | Field::Css | Field::JsFunctions) => {
                        self.start_string(prefix, StringUse::Value(field));
                    }
                    b'[' if matches!(field, Field::Html | Field::JsExpressions) => {
                        self.cursor += 1;
                        self.array = Some((field, ArrayState::ValueOrEnd));
                    }
                    b'n' if field == Field::InitialHeight => {
                        if !self.scalar(prefix, field)? {
                            self.scanned = prefix.len();
                            return Ok(());
                        }
                    }
                    b'-' | b'0'..=b'9' if field == Field::InitialHeight => {
                        if !self.scalar(prefix, field)? {
                            self.scanned = prefix.len();
                            return Ok(());
                        }
                    }
                    _ => return Err("invalid generated field value".into()),
                },
                ObjectState::CommaOrEnd => match prefix.as_bytes()[self.cursor] {
                    b',' => {
                        self.cursor += 1;
                        self.object = ObjectState::KeyOrEnd;
                    }
                    b'}' if self.seen == 0b11_1111 => {
                        self.cursor += 1;
                        self.object = ObjectState::Done;
                    }
                    _ => return Err("invalid generated object separator".into()),
                },
                ObjectState::Done => {
                    self.whitespace(prefix);
                    if self.cursor != prefix.len() {
                        return Err("trailing generated argument data".into());
                    }
                    break;
                }
            }
        }
        self.scanned = prefix.len();
        Ok(())
    }

    fn scan_string(&mut self, prefix: &str) -> Result<bool, String> {
        let Some((start, mut at, mut escaped, use_)) = self.pending_string else {
            return Ok(false);
        };
        let bytes = prefix.as_bytes();
        while at < bytes.len() {
            match bytes[at] {
                b'"' if !escaped => {
                    let end = at + 1;
                    let value: String = serde_json::from_str(&prefix[start..end])
                        .map_err(|_| "invalid generated JSON string")?;
                    self.pending_string = None;
                    self.cursor = end;
                    self.string_value(use_, value)?;
                    return Ok(true);
                }
                b'\\' if !escaped => escaped = true,
                _ => escaped = false,
            }
            at += 1;
        }
        self.pending_string = Some((start, at, escaped, use_));
        Ok(false)
    }

    fn start_string(&mut self, _prefix: &str, use_: StringUse) {
        let start = self.cursor;
        self.cursor += 1;
        self.pending_string = Some((start, self.cursor, false, use_));
    }

    fn string_value(&mut self, use_: StringUse, value: String) -> Result<(), String> {
        match use_ {
            StringUse::Key => {
                let field = match value.as_str() {
                    "surfaceId" => Field::SurfaceId,
                    "initialHeight" => Field::InitialHeight,
                    "css" => Field::Css,
                    "html" => Field::Html,
                    "jsFunctions" => Field::JsFunctions,
                    "jsExpressions" => Field::JsExpressions,
                    _ => return Err("unknown generated argument field".into()),
                };
                let bit = bit(field);
                if self.seen & bit != 0 {
                    return Err("duplicate generated argument field".into());
                }
                self.seen |= bit;
                self.object = ObjectState::Colon;
                self.pending_field = Some(field);
            }
            StringUse::Value(field) => {
                match field {
                    Field::SurfaceId => {
                        if value.is_empty()
                            || value.len() > 256
                            || value.chars().any(char::is_control)
                        {
                            return Err("invalid generated surface id".into());
                        }
                        self.surface_id = Some(value);
                    }
                    Field::Css => {
                        self.apply(GeneratedUpdate::Css(value))?;
                        self.apply(GeneratedUpdate::CssComplete)?;
                    }
                    Field::JsFunctions => {
                        self.apply(GeneratedUpdate::Functions(value))?;
                        self.apply(GeneratedUpdate::FunctionsComplete)?;
                    }
                    _ => return Err("invalid generated string field".into()),
                }
                self.object = ObjectState::CommaOrEnd;
            }
            StringUse::Element(field) => {
                self.apply(match field {
                    Field::Html => GeneratedUpdate::Html(value),
                    Field::JsExpressions => GeneratedUpdate::Expression(value),
                    _ => return Err("invalid generated array field".into()),
                })?;
                self.array = Some((field, ArrayState::CommaOrEnd));
            }
        }
        Ok(())
    }

    fn scalar(&mut self, prefix: &str, field: Field) -> Result<bool, String> {
        let start = self.cursor;
        let bytes = prefix.as_bytes();
        let mut end = start;
        while end < bytes.len() && !matches!(bytes[end], b',' | b'}' | b' ' | b'\n' | b'\r' | b'\t')
        {
            end += 1;
            if end - start > 64 {
                return Err("generated height token exceeds its limit".into());
            }
        }
        if end == bytes.len() {
            return Ok(false);
        }
        let value: Value = serde_json::from_str(&prefix[start..end])
            .map_err(|_| "invalid generated initial height")?;
        match value {
            Value::Null => {}
            Value::Number(number) => {
                let height =
                    AnswerHeight::new(number.as_f64().ok_or("invalid generated initial height")?)?;
                self.apply(GeneratedUpdate::Height(height))?;
            }
            _ => return Err("invalid generated initial height".into()),
        }
        self.cursor = end;
        self.object = ObjectState::CommaOrEnd;
        let _ = field;
        Ok(true)
    }

    fn apply(&mut self, update: GeneratedUpdate) -> Result<(), String> {
        self.content = self.content.updated(update)?;
        Ok(())
    }

    fn current_field(&mut self) -> Result<Field, String> {
        self.pending_field
            .take()
            .ok_or_else(|| "missing generated field".into())
    }

    fn whitespace(&mut self, prefix: &str) {
        while self.cursor < prefix.len()
            && matches!(prefix.as_bytes()[self.cursor], b' ' | b'\n' | b'\r' | b'\t')
        {
            self.cursor += 1;
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawArguments {
    surface_id: String,
    initial_height: Value,
    css: String,
    html: Vec<String>,
    js_functions: String,
    js_expressions: Vec<String>,
}

fn from_raw(raw: &RawArguments) -> Result<GeneratedAnswer, String> {
    let mut answer = GeneratedAnswer::default();
    if !raw.initial_height.is_null() {
        let height = raw
            .initial_height
            .as_f64()
            .ok_or("invalid generated initial height")?;
        answer = answer.updated(GeneratedUpdate::Height(AnswerHeight::new(height)?))?;
    }
    answer = answer.updated(GeneratedUpdate::Css(raw.css.clone()))?;
    answer = answer.updated(GeneratedUpdate::CssComplete)?;
    for html in &raw.html {
        answer = answer.updated(GeneratedUpdate::Html(html.clone()))?;
    }
    answer = answer.updated(GeneratedUpdate::HtmlComplete)?;
    answer = answer.updated(GeneratedUpdate::Functions(raw.js_functions.clone()))?;
    answer = answer.updated(GeneratedUpdate::FunctionsComplete)?;
    for expression in &raw.js_expressions {
        answer = answer.updated(GeneratedUpdate::Expression(expression.clone()))?;
    }
    answer = answer.updated(GeneratedUpdate::ExpressionsComplete)?;
    Ok(answer)
}

fn bit(field: Field) -> u8 {
    match field {
        Field::SurfaceId => 1,
        Field::InitialHeight => 2,
        Field::Css => 4,
        Field::Html => 8,
        Field::JsFunctions => 16,
        Field::JsExpressions => 32,
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn accepts_every_character_split_and_publishes_only_closed_values() {
        let raw = r#"{"css":"x","html":["<p>\uD83D\uDE00</p>"],"jsFunctions":"function f(){\"\\uD83D\\uDE00\"}","jsExpressions":["f()"],"initialHeight":1,"surfaceId":"surface"}"#;
        let mut decoder = GeneratedArguments::new(262_144);
        let mut last = None;
        let mut offset = 0;
        for ch in raw.chars() {
            offset += ch.len_utf8();
            last = decoder.push(&raw[..offset]).unwrap().or(last);
        }
        let progress = last.unwrap();
        assert_eq!(progress.surface_id, "surface");
        let view = serde_json::to_value(progress.content.view()).unwrap();
        assert_eq!(view["html"], serde_json::json!(["<p>😀</p>"]));
        assert!(view["jsFunctions"].is_string());
        let (surface, complete) = decoder.finish(raw).unwrap();
        assert_eq!(surface, "surface");
        assert_eq!(
            serde_json::to_value(complete.view()).unwrap()["generating"],
            false
        );
    }

    #[test]
    fn waits_for_identity_and_rejects_bad_or_poisoned_documents() {
        let raw = r#"{"html":["<p>ok</p>"],"jsExpressions":["one()","two()"],"surfaceId":"late","initialHeight":null,"jsFunctions":"return 1","css":""}"#;
        let mut decoder = GeneratedArguments::new(262_144);
        assert!(decoder.push(r#"{"html":["<p>ok</p>"]"#).unwrap().is_none());
        let progress = decoder.push(raw).unwrap().unwrap();
        let view = serde_json::to_value(progress.content.view()).unwrap();
        assert_eq!(view["jsExpressions"], serde_json::json!(["one()", "two()"]));

        for invalid in [
            r#"{"surfaceId":"x","surfaceId":"y","initialHeight":null,"css":"","html":["x"],"jsFunctions":"","jsExpressions":[]}"#,
            r#"{"surfaceId":"x","initialHeight":null,"css":"","html":["x"],"jsFunctions":"","jsExpressions":[],"extra":1}"#,
            r#"{"surfaceId":"x","initialHeight":null,"css":"","html":["x"],"jsFunctions":"","jsExpressions":[]} trailing"#,
        ] {
            let mut decoder = GeneratedArguments::new(262_144);
            assert!(decoder.push(invalid).is_err());
            assert!(decoder.push(invalid).is_err());
        }
        let mut bounded = GeneratedArguments::new(2);
        assert!(bounded.push("{} ").is_err());

        let emitted = r#"{"surfaceId":"one","initialHeight":null,"css":"","html":["x"],"jsFunctions":"","jsExpressions":[]}"#;
        let different_id = emitted.replace("one", "two");
        let mut decoder = GeneratedArguments::new(262_144);
        assert!(decoder.push(emitted).unwrap().is_some());
        assert!(decoder.finish(&different_id).is_err());
    }

    #[test]
    fn code_is_hidden_until_its_json_string_closes() {
        let mut decoder = GeneratedArguments::new(262_144);
        let partial = r#"{"surfaceId":"s","initialHeight":null,"css":"","html":["<p>x</p>"],"jsFunctions":"function f(){"#;
        let progress = decoder.push(partial).unwrap().unwrap();
        assert!(
            serde_json::to_value(progress.content.view())
                .unwrap()
                .get("jsFunctions")
                .is_none()
        );

        let full = format!(r#"{partial}}}","jsExpressions":[]}}"#);
        let progress = decoder.push(&full).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(progress.content.view()).unwrap()["jsFunctions"],
            "function f(){}"
        );
        decoder.finish(&full).unwrap();
    }
}
