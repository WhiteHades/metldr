use crate::{Result, invalid};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

pub struct Permission {
    pub(crate) origin: Arc<()>,
    pub(crate) identity: Arc<()>,
    pub(crate) id: Value,
    pub(crate) key: String,
    details: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_choices_remain_answerable_after_an_invalid_selection() {
        let value = json!({"id":7,"params":{"sessionId":"ses_owned","toolCall":{},
            "options":[{"optionId":"once","kind":"allow_once","name":"Allow once"}]}});
        let permission = Permission::parse(&value, "ses_owned", Arc::new(())).unwrap();
        assert!(permission.is_pending(&permission.origin, Some(&permission.identity)));
        assert!(!permission.is_pending(&Arc::new(()), Some(&permission.identity)));
        assert!(!permission.is_pending(&permission.origin, None));
        let reused = Permission::parse(&value, "ses_owned", permission.origin.clone()).unwrap();
        assert!(!permission.is_pending(&permission.origin, Some(&reused.identity)));
        assert!(permission.response(Some("unoffered")).is_err());
        assert_eq!(
            permission.response(Some("once")).unwrap(),
            json!({"outcome":{"outcome":"selected","optionId":"once"}})
        );
        assert_eq!(
            permission.response(None).unwrap(),
            json!({"outcome":{"outcome":"cancelled"}})
        );
        assert!(Permission::parse(&value, "another_session", Arc::new(())).is_err());
        let mut duplicate = value;
        duplicate["params"]["options"]
            .as_array_mut()
            .unwrap()
            .push(json!({"optionId":"once","kind":"reject_once","name":"Reject"}));
        assert!(Permission::parse(&duplicate, "ses_owned", Arc::new(())).is_err());
    }
}

pub(crate) fn request_key(id: &Value) -> Result<String> {
    if id.as_u64().is_none() && id.as_str().is_none_or(|s| s.is_empty() || s.len() > 128) {
        return Err(invalid("invalid OpenCode server request identity"));
    }
    Ok(serde_json::to_string(id)?)
}

impl Permission {
    pub(crate) fn parse(value: &Value, session: &str, origin: Arc<()>) -> Result<Self> {
        let details = &value["params"];
        let options = details["options"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 16)
            .ok_or_else(|| invalid("OpenCode permission choices missing or oversized"))?;
        let mut ids = BTreeSet::new();
        if details["sessionId"] != session
            || !details["toolCall"].is_object()
            || options.iter().any(|option| {
                option["optionId"]
                    .as_str()
                    .is_none_or(|s| s.is_empty() || s.len() > 128 || !ids.insert(s))
                    || option["name"]
                        .as_str()
                        .is_none_or(|s| s.is_empty() || s.len() > 256)
                    || !matches!(
                        option["kind"].as_str(),
                        Some("allow_once" | "allow_always" | "reject_once" | "reject_always")
                    )
            })
        {
            return Err(invalid(
                "OpenCode permission metadata is malformed or belongs to another session",
            ));
        }
        Ok(Self {
            origin,
            identity: Arc::new(()),
            id: value["id"].clone(),
            key: request_key(&value["id"])?,
            details: details.clone(),
        })
    }
    pub fn details(&self) -> &Value {
        &self.details
    }
    pub(crate) fn is_pending(&self, origin: &Arc<()>, identity: Option<&Arc<()>>) -> bool {
        Arc::ptr_eq(&self.origin, origin)
            && identity.is_some_and(|id| Arc::ptr_eq(&self.identity, id))
    }
    pub(crate) fn response(&self, option: Option<&str>) -> Result<Value> {
        let outcome = match option {
            Some(id)
                if self.details["options"]
                    .as_array()
                    .is_some_and(|options| options.iter().any(|v| v["optionId"] == id)) =>
            {
                json!({"outcome":"selected","optionId":id})
            }
            Some(_) => return Err(invalid("OpenCode permission choice was not offered")),
            None => json!({"outcome":"cancelled"}),
        };
        Ok(json!({"outcome":outcome}))
    }
}
