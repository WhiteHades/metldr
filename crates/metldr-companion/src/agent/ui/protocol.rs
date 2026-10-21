use serde::{Deserialize, Serialize};
use serde_json::{Number, Value, json};

pub(super) const COMMAND_MAX: usize = 32 * 1024;

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Dialog {
    Select {
        title: String,
        options: Vec<String>,
    },
    Confirm {
        title: String,
        message: String,
    },
    Input {
        title: String,
        placeholder: Option<String>,
    },
    Editor {
        title: String,
        prefill: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Answer {
    Select(String),
    Confirm(bool),
    Text(String),
    Cancel,
}

impl Dialog {
    pub(crate) fn accepts(&self, answer: &Answer) -> bool {
        match (self, answer) {
            (Self::Select { options, .. }, Answer::Select(value)) => {
                options.iter().any(|option| option == value)
            }
            (Self::Confirm { .. }, Answer::Confirm(_)) => true,
            (Self::Input { .. } | Self::Editor { .. }, Answer::Text(_)) => true,
            (_, Answer::Cancel) => true,
            _ => false,
        }
    }
}

pub(super) struct Request {
    pub id: String,
    pub timeout: Option<Number>,
    pub kind: Kind,
}

pub(super) enum Kind {
    Dialog(Dialog),
    Notify {
        message: String,
        level: String,
    },
    Status {
        key: String,
        text: Option<String>,
    },
    Widget {
        key: String,
        lines: Option<Vec<String>>,
        placement: String,
    },
    Title(String),
    EditorText(String),
}

pub(super) fn parse(value: &Value) -> Result<Request, &'static str> {
    if serde_json::to_vec(value)
        .map_err(|_| "invalid native UI request")?
        .len()
        > COMMAND_MAX
    {
        return Err("native UI request exceeds command limit");
    }
    if value["type"] != "extension_ui_request" {
        return Err("invalid native UI request type");
    }
    let id = string(value, "id")?.to_owned();
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err("invalid native UI identity");
    }
    let timeout = value
        .get("timeout")
        .map(|v| {
            let number = v.as_number().ok_or("invalid native UI timeout")?;
            if !matches!(number.as_f64(), Some(millis) if millis.is_finite()) {
                return Err("invalid native UI timeout");
            }
            Ok(number.clone())
        })
        .transpose()?;
    let method = string(value, "method")?;
    let kind = match method {
        "select" => Kind::Dialog(Dialog::Select {
            title: string(value, "title")?.to_owned(),
            options: strings(value, "options")?,
        }),
        "confirm" => Kind::Dialog(Dialog::Confirm {
            title: string(value, "title")?.to_owned(),
            message: string(value, "message")?.to_owned(),
        }),
        "input" => Kind::Dialog(Dialog::Input {
            title: string(value, "title")?.to_owned(),
            placeholder: optional_string(value, "placeholder")?,
        }),
        "editor" => Kind::Dialog(Dialog::Editor {
            title: string(value, "title")?.to_owned(),
            prefill: optional_string(value, "prefill")?,
        }),
        "notify" => Kind::Notify {
            message: string(value, "message")?.to_owned(),
            level: optional_choice(value, "notifyType", &["info", "warning", "error"])?
                .unwrap_or_else(|| "info".to_owned()),
        },
        "setStatus" => Kind::Status {
            key: string(value, "statusKey")?.to_owned(),
            text: optional_string(value, "statusText")?,
        },
        "setWidget" => Kind::Widget {
            key: string(value, "widgetKey")?.to_owned(),
            lines: value
                .get("widgetLines")
                .map(|_| strings(value, "widgetLines"))
                .transpose()?,
            placement: optional_choice(value, "widgetPlacement", &["aboveEditor", "belowEditor"])?
                .unwrap_or_else(|| "aboveEditor".to_owned()),
        },
        "setTitle" => Kind::Title(string(value, "title")?.to_owned()),
        "set_editor_text" => Kind::EditorText(string(value, "text")?.to_owned()),
        _ => return Err("unsupported native UI method"),
    };
    Ok(Request { id, timeout, kind })
}

pub(super) fn encode_reply(
    id: &str,
    dialog: &Dialog,
    answer: Answer,
) -> Result<Value, &'static str> {
    let body = match (dialog, answer) {
        (Dialog::Select { options, .. }, Answer::Select(value))
            if options.iter().any(|item| item == &value) =>
        {
            json!({"type":"extension_ui_response", "id":id, "value":value})
        }
        (Dialog::Confirm { .. }, Answer::Confirm(confirmed)) => {
            json!({"type":"extension_ui_response", "id":id, "confirmed":confirmed})
        }
        (Dialog::Input { .. } | Dialog::Editor { .. }, Answer::Text(value)) => {
            json!({"type":"extension_ui_response", "id":id, "value":value})
        }
        (_, Answer::Cancel) => {
            json!({"type":"extension_ui_response", "id":id, "cancelled":true})
        }
        _ => return Err("native UI answer does not match request"),
    };
    let bytes = serde_json::to_vec(&body).map_err(|_| "invalid native UI response")?;
    if bytes.len() > COMMAND_MAX {
        return Err("native UI response exceeds command limit");
    }
    Ok(body)
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or("invalid native UI field")
}

fn optional_string(value: &Value, key: &str) -> Result<Option<String>, &'static str> {
    value
        .get(key)
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("invalid native UI field")
        })
        .transpose()
}

fn strings(value: &Value, key: &str) -> Result<Vec<String>, &'static str> {
    value
        .get(key)
        .and_then(Value::as_array)
        .ok_or("invalid native UI field")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("invalid native UI field")
        })
        .collect()
}

fn optional_choice(
    value: &Value,
    key: &str,
    choices: &[&str],
) -> Result<Option<String>, &'static str> {
    let Some(value) = value.get(key) else {
        return Ok(None);
    };
    let value = value.as_str().ok_or("invalid native UI field")?;
    if !choices.contains(&value) {
        return Err("invalid native UI field");
    }
    Ok(Some(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ui_protocol_keeps_exact_values_and_rejects_wrong_choices() {
        let methods = [
            json!({"type":"extension_ui_request", "id":"a", "method":"select", "title":"T", "options":["A"]}),
            json!({"type":"extension_ui_request", "id":"b", "method":"confirm", "title":"T", "message":"M"}),
            json!({"type":"extension_ui_request", "id":"c", "method":"input", "title":"T"}),
            json!({"type":"extension_ui_request", "id":"d", "method":"editor", "title":"T"}),
            json!({"type":"extension_ui_request", "id":"e", "method":"notify", "message":"M"}),
            json!({"type":"extension_ui_request", "id":"f", "method":"setStatus", "statusKey":"k"}),
            json!({"type":"extension_ui_request", "id":"g", "method":"setWidget", "widgetKey":"k"}),
            json!({"type":"extension_ui_request", "id":"h", "method":"setTitle", "title":"T"}),
            json!({"type":"extension_ui_request", "id":"i", "method":"set_editor_text", "text":"T"}),
        ];
        let parsed: Vec<_> = methods.iter().map(|value| parse(value).unwrap()).collect();
        assert!(matches!(
            &parsed[0].kind,
            Kind::Dialog(Dialog::Select { .. })
        ));
        assert!(matches!(
            &parsed[1].kind,
            Kind::Dialog(Dialog::Confirm { .. })
        ));
        assert!(matches!(
            &parsed[2].kind,
            Kind::Dialog(Dialog::Input { .. })
        ));
        assert!(matches!(
            &parsed[3].kind,
            Kind::Dialog(Dialog::Editor { .. })
        ));
        assert!(matches!(&parsed[4].kind, Kind::Notify { .. }));
        assert!(matches!(&parsed[5].kind, Kind::Status { .. }));
        assert!(matches!(&parsed[6].kind, Kind::Widget { .. }));
        assert!(matches!(&parsed[7].kind, Kind::Title(_)));
        assert!(matches!(&parsed[8].kind, Kind::EditorText(_)));

        let request = parse(&json!({
            "type":"extension_ui_request", "id":"select", "method":"select",
            "title":"T", "options":["", "same", "same"]
        }))
        .unwrap();
        let Kind::Dialog(dialog) = request.kind else {
            panic!("expected dialog");
        };
        assert!(dialog.accepts(&Answer::Select(String::new())));
        assert!(dialog.accepts(&Answer::Select("same".into())));
        assert!(!dialog.accepts(&Answer::Select("unoffered".into())));
        assert_eq!(
            encode_reply("select", &dialog, Answer::Select(String::new())).unwrap(),
            json!({"type":"extension_ui_response", "id":"select", "value":""})
        );
        assert_eq!(
            encode_reply("select", &dialog, Answer::Cancel).unwrap(),
            json!({"type":"extension_ui_response", "id":"select", "cancelled":true})
        );
        let request = parse(&json!({
            "type":"extension_ui_request", "id":"input", "method":"input", "title":"T"
        }))
        .unwrap();
        let Kind::Dialog(dialog) = request.kind else {
            panic!("expected dialog");
        };
        assert_eq!(
            encode_reply("input", &dialog, Answer::Text("雪".into())).unwrap()["value"],
            "雪"
        );
        assert!(matches!(
            parse(&json!({"type":"extension_ui_request", "id":"editor", "method":"editor", "title":"T", "timeout":0})).unwrap().timeout,
            Some(ref n) if n.as_u64() == Some(0)
        ));
    }
}
