use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_GENERATED_BYTES: usize = 262_144;
const MAX_FRAGMENTS: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct AnswerHeight(u16);

impl AnswerHeight {
    pub fn new(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err("answer height must be finite".into());
        }
        Ok(Self(value.ceil().clamp(50.0, 4_000.0) as u16))
    }

    pub fn pixels(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for AnswerHeight {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(f64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(
    tag = "state",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum Section<T> {
    #[default]
    Absent,
    Open(T),
    Complete(T),
}

impl<T> Section<T> {
    fn value(&self) -> Option<&T> {
        match self {
            Self::Absent => None,
            Self::Open(value) | Self::Complete(value) => Some(value),
        }
    }

    fn complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }
}

impl<T: Default> Section<T> {
    fn open(&mut self) -> Result<&mut T, String> {
        if matches!(self, Self::Absent) {
            *self = Self::Open(T::default());
        }
        match self {
            Self::Open(value) => Ok(value),
            _ => Err("completed answer content cannot change".into()),
        }
    }

    fn close(&mut self) {
        let value = std::mem::take(self);
        *self = match value {
            Self::Absent => Self::Complete(T::default()),
            Self::Open(value) | Self::Complete(value) => Self::Complete(value),
        };
    }

    fn finish_absent(&mut self) {
        if matches!(self, Self::Absent) {
            self.close();
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedAnswer {
    initial_height: Option<AnswerHeight>,
    css: Section<String>,
    html: Section<Vec<String>>,
    js_functions: Section<String>,
    js_expressions: Section<Vec<String>>,
}

pub enum GeneratedUpdate {
    Height(AnswerHeight),
    Css(String),
    CssComplete,
    Html(String),
    HtmlComplete,
    Functions(String),
    FunctionsComplete,
    Expression(String),
    ExpressionsComplete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CompleteSource(GeneratedAnswer);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerView<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_height: Option<AnswerHeight>,
    pub generating: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub css: Option<&'a str>,
    pub css_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<&'a [String]>,
    pub html_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_functions: Option<&'a str>,
    pub js_functions_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_expressions: Option<&'a [String]>,
    pub js_expressions_complete: bool,
}

impl GeneratedAnswer {
    pub fn updated(&self, update: GeneratedUpdate) -> Result<Self, String> {
        if matches!(&update, GeneratedUpdate::Css(text) | GeneratedUpdate::Html(text)
            | GeneratedUpdate::Functions(text) | GeneratedUpdate::Expression(text)
            if text.len() > MAX_GENERATED_BYTES)
        {
            return Err("answer update exceeds its byte limit".into());
        }
        let mut next = self.clone();
        match update {
            GeneratedUpdate::Height(height) => match next.initial_height {
                Some(previous) if previous != height => {
                    return Err("answer initial height changed".into());
                }
                _ => next.initial_height = Some(height),
            },
            GeneratedUpdate::Css(text) => next.css.open()?.push_str(&text),
            GeneratedUpdate::CssComplete => next.css.close(),
            GeneratedUpdate::Html(text) => next.html.open()?.push(text),
            GeneratedUpdate::HtmlComplete => next.html.close(),
            GeneratedUpdate::Functions(text) => next.js_functions.open()?.push_str(&text),
            GeneratedUpdate::FunctionsComplete => next.js_functions.close(),
            GeneratedUpdate::Expression(text) => next.js_expressions.open()?.push(text),
            GeneratedUpdate::ExpressionsComplete => next.js_expressions.close(),
        }
        next.validate()?;
        Ok(next)
    }

    pub fn view(&self) -> AnswerView<'_> {
        self.rendering(true)
    }

    pub fn extends(&self, previous: &Self) -> bool {
        fn section<T: PartialEq>(
            next: &Section<T>,
            previous: &Section<T>,
            prefix: impl Fn(&T, &T) -> bool,
        ) -> bool {
            match (previous, next) {
                (Section::Absent, _) => true,
                (Section::Open(old), Section::Open(new) | Section::Complete(new)) => {
                    prefix(new, old)
                }
                (Section::Complete(old), Section::Complete(new)) => old == new,
                _ => false,
            }
        }
        previous
            .initial_height
            .is_none_or(|height| self.initial_height == Some(height))
            && section(&self.css, &previous.css, |new, old| new.starts_with(old))
            && section(&self.html, &previous.html, |new, old| new.starts_with(old))
            && section(&self.js_functions, &previous.js_functions, |new, old| {
                new.starts_with(old)
            })
            && section(
                &self.js_expressions,
                &previous.js_expressions,
                |new, old| new.starts_with(old),
            )
    }

    pub fn finish(mut self) -> Result<CompleteSource, String> {
        self.css.finish_absent();
        self.html.finish_absent();
        self.js_functions.finish_absent();
        self.js_expressions.finish_absent();
        self.require_complete()?;
        self.validate()?;
        Ok(CompleteSource(self))
    }

    fn require_complete(&self) -> Result<(), String> {
        if !self.css.complete()
            || !self.html.complete()
            || !self.js_functions.complete()
            || !self.js_expressions.complete()
        {
            return Err("answer contains an unfinished source section".into());
        }
        if self
            .html
            .value()
            .is_none_or(|fragments| !fragments.iter().any(|text| !text.trim().is_empty()))
        {
            return Err("answer HTML is empty".into());
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self
            .html
            .value()
            .is_some_and(|items| items.len() > MAX_FRAGMENTS)
            || self
                .js_expressions
                .value()
                .is_some_and(|items| items.len() > MAX_FRAGMENTS)
        {
            return Err("answer exceeds its source fragment limit".into());
        }
        if serde_json::to_vec(self)
            .map_err(|_| "could not encode answer")?
            .len()
            > MAX_GENERATED_BYTES
            || serde_json::to_vec(&self.view())
                .map_err(|_| "could not encode answer view")?
                .len()
                > MAX_GENERATED_BYTES
        {
            return Err("answer exceeds its encoded byte limit".into());
        }
        Ok(())
    }

    fn rendering(&self, generating: bool) -> AnswerView<'_> {
        AnswerView {
            initial_height: self.initial_height,
            generating,
            css: self.css.value().map(String::as_str),
            css_complete: self.css.complete(),
            html: self.html.value().map(Vec::as_slice),
            html_complete: self.html.complete(),
            js_functions: match &self.js_functions {
                Section::Complete(value) => Some(value.as_str()),
                _ => None,
            },
            js_functions_complete: self.js_functions.complete(),
            js_expressions: self.js_expressions.value().map(Vec::as_slice),
            js_expressions_complete: self.js_expressions.complete(),
        }
    }
}

impl CompleteSource {
    pub fn view(&self) -> AnswerView<'_> {
        self.0.rendering(false)
    }

    pub fn into_answer(self) -> GeneratedAnswer {
        self.0
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawAnswer {
    initial_height: Option<AnswerHeight>,
    css: Section<String>,
    html: Section<Vec<String>>,
    js_functions: Section<String>,
    js_expressions: Section<Vec<String>>,
}

impl<'de> Deserialize<'de> for GeneratedAnswer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawAnswer::deserialize(deserializer)?;
        let answer = Self {
            initial_height: raw.initial_height,
            css: raw.css,
            html: raw.html,
            js_functions: raw.js_functions,
            js_expressions: raw.js_expressions,
        };
        answer.validate().map_err(serde::de::Error::custom)?;
        Ok(answer)
    }
}

impl<'de> Deserialize<'de> for CompleteSource {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let answer = GeneratedAnswer::deserialize(deserializer)?;
        answer
            .require_complete()
            .map_err(serde::de::Error::custom)?;
        Ok(Self(answer))
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn source_sections_stream_in_order_and_only_complete_code_is_exposed() {
        let mut answer = GeneratedAnswer::default();
        for update in [
            GeneratedUpdate::Height(AnswerHeight::new(300.1).unwrap()),
            GeneratedUpdate::Css("body{color:#123}".into()),
            GeneratedUpdate::CssComplete,
            GeneratedUpdate::Html("<main><output id='total'>".into()),
            GeneratedUpdate::Html("17</output></main>".into()),
            GeneratedUpdate::HtmlComplete,
            GeneratedUpdate::Functions("function total(){".into()),
        ] {
            answer = answer.updated(update).unwrap();
        }
        let preview = serde_json::to_value(answer.view()).unwrap();
        assert_eq!(preview["initialHeight"], 301);
        assert_eq!(
            preview["html"],
            json!(["<main><output id='total'>", "17</output></main>"])
        );
        assert!(preview.get("jsFunctions").is_none());
        assert_eq!(preview["generating"], true);
        assert!(answer.clone().finish().is_err());
        answer = answer
            .updated(GeneratedUpdate::Functions("return 17}".into()))
            .unwrap();
        answer = answer.updated(GeneratedUpdate::FunctionsComplete).unwrap();
        answer = answer
            .updated(GeneratedUpdate::Expression("total()".into()))
            .unwrap();
        assert_eq!(
            serde_json::to_value(answer.view()).unwrap()["jsFunctions"],
            "function total(){return 17}"
        );
        answer = answer
            .updated(GeneratedUpdate::ExpressionsComplete)
            .unwrap();
        let encoded = serde_json::to_string(&answer).unwrap();
        let restored: GeneratedAnswer = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored, answer);
        let complete = restored.finish().unwrap();
        assert_eq!(complete.clone().into_answer(), answer);
        let view = serde_json::to_value(complete.view()).unwrap();
        assert_eq!(view["generating"], false);
        assert_eq!(view["jsExpressions"], json!(["total()"]));
        let saved = serde_json::to_string(&complete).unwrap();
        assert_eq!(
            serde_json::from_str::<CompleteSource>(&saved).unwrap(),
            complete
        );
        let mut corrupt: Value = serde_json::from_str(&saved).unwrap();
        corrupt["html"]["state"] = json!("open");
        assert!(serde_json::from_value::<CompleteSource>(corrupt).is_err());
    }

    #[test]
    fn rejected_updates_preserve_content_and_stored_source_is_validated() {
        let mut answer = GeneratedAnswer::default()
            .updated(GeneratedUpdate::Html("<p>Ready</p>".into()))
            .unwrap();
        answer = answer.updated(GeneratedUpdate::HtmlComplete).unwrap();
        let saved = answer.clone();
        assert!(
            answer
                .updated(GeneratedUpdate::Html("replaced".into()))
                .is_err()
        );
        assert!(
            answer
                .updated(GeneratedUpdate::Css("\\".repeat(MAX_GENERATED_BYTES)))
                .is_err()
        );
        assert_eq!(answer, saved);
        assert!(answer.extends(&GeneratedAnswer::default()));
        assert!(!GeneratedAnswer::default().extends(&answer));
        let changed = GeneratedAnswer::default()
            .updated(GeneratedUpdate::Html("<p>Different</p>".into()))
            .unwrap()
            .updated(GeneratedUpdate::HtmlComplete)
            .unwrap();
        assert!(!changed.extends(&answer));
        assert_eq!(
            answer.updated(GeneratedUpdate::HtmlComplete).unwrap(),
            saved
        );
        assert!(AnswerHeight::new(f64::NAN).is_err());
        assert!(AnswerHeight::new(f64::INFINITY).is_err());
        assert_eq!(AnswerHeight::new(-3.0).unwrap().pixels(), 50);
        assert_eq!(AnswerHeight::new(5_000.0).unwrap().pixels(), 4_000);
        let raw = serde_json::to_value(&answer).unwrap();
        let mut unknown = raw.clone();
        unknown["unexpected"] = json!(true);
        assert!(serde_json::from_value::<GeneratedAnswer>(unknown).is_err());
        let mut too_many = raw.clone();
        too_many["html"]["value"] = json!(vec!["x"; MAX_FRAGMENTS + 1]);
        assert!(serde_json::from_value::<GeneratedAnswer>(too_many).is_err());
        let mut hidden = raw;
        hidden["jsFunctions"] = json!({"state":"open","value":"x".repeat(MAX_GENERATED_BYTES)});
        assert!(serde_json::from_value::<GeneratedAnswer>(hidden).is_err());
        assert!(GeneratedAnswer::default().finish().is_err());
        let static_answer = saved.finish().unwrap();
        assert_eq!(
            serde_json::to_value(static_answer.view()).unwrap()["jsFunctions"],
            ""
        );
    }
}
