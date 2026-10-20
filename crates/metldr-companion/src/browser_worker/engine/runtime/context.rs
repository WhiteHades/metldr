use super::{CAPACITY, RawEvent, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const MAX_FRAMES: usize = 4096;
const ROOT_SESSION: &str = "metldr-owned-root";
pub const AUTOMATION_WORLD: &str = "metldr-browser-automation";
type SessionKey = Option<String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameContext {
    generation: [u8; 16],
    target: String,
    frame: String,
    session: SessionKey,
    unique: String,
}

impl FrameContext {
    pub fn target_id(&self) -> &str {
        &self.target
    }
    pub fn frame_id(&self) -> &str {
        &self.frame
    }
    pub fn session_id(&self) -> &str {
        self.session.as_deref().unwrap_or(ROOT_SESSION)
    }
    pub fn unique_context_id(&self) -> &str {
        &self.unique
    }
}

pub struct ContextMap {
    generation: [u8; 16],
    target: String,
    main: Option<String>,
    sessions: HashMap<SessionKey, Session>,
    frames: HashMap<String, Frame>,
    provisional: HashMap<(SessionKey, String), Execution>,
}

struct Session {
    parent: SessionKey,
    parent_frame: Option<String>,
    page: bool,
    ready: bool,
    root_frame: Option<String>,
}

struct Frame {
    parent: Option<String>,
    session: SessionKey,
    loader: String,
    url: String,
    context: Option<Execution>,
}

#[derive(Clone)]
struct Execution {
    numeric: i64,
    unique: String,
}

pub enum Change {
    None,
    Attached { session: String, page: bool },
    CreateWorld { session: SessionKey, frame: String },
}

impl ContextMap {
    pub fn new(target: &str) -> Result<Self> {
        identifier(target)?;
        let mut generation = [0; 16];
        getrandom::fill(&mut generation)
            .map_err(|_| "cannot create browser attachment identity")?;
        Ok(Self {
            generation,
            target: target.into(),
            main: None,
            sessions: HashMap::from([(
                None,
                Session {
                    parent: None,
                    parent_frame: None,
                    page: true,
                    ready: false,
                    root_frame: None,
                },
            )]),
            frames: HashMap::new(),
            provisional: HashMap::new(),
        })
    }

    pub fn ready(&mut self, session: Option<&str>) -> Result<()> {
        self.session_mut(session)?.ready = true;
        Ok(())
    }

    pub fn tree(&mut self, session: Option<&str>, tree: &Value) -> Result<()> {
        let record = self.session(session)?;
        if !record.page {
            return Err("nonpage session supplied a frame tree".into());
        }
        let mut stack = vec![(tree, record.parent_frame.clone())];
        let mut visited = HashSet::new();
        while let Some((tree, parent)) = stack.pop() {
            if !tree.is_object() || visited.len() >= MAX_FRAMES {
                return Err("invalid or excessive browser frame tree".into());
            }
            let frame = text(&tree["frame"], "id")?;
            if !visited.insert(frame) {
                return Err("duplicate browser frame tree identity".into());
            }
            self.frame(session, &tree["frame"], parent.as_deref())?;
            if let Some(children) = tree.get("childFrames") {
                for child in children
                    .as_array()
                    .ok_or("invalid child frame list")?
                    .iter()
                    .rev()
                {
                    stack.push((child, Some(frame.to_owned())));
                }
            }
        }
        let root = text(&tree["frame"], "id")?.to_owned();
        self.session_mut(session)?.root_frame = Some(root);
        Ok(())
    }

    pub fn event(&mut self, event: &RawEvent) -> Result<Change> {
        event.validate()?;
        let session = event.session.as_deref();
        self.session(session)?;
        let params = &event.params;
        match event.method.as_ref() {
            "Target.attachedToTarget" => {
                let child = text(params, "sessionId")?.to_owned();
                let info = params
                    .get("targetInfo")
                    .filter(|v| v.is_object())
                    .ok_or("missing attached target")?;
                identifier(text(info, "targetId")?)?;
                let page = text(info, "type")? == "iframe";
                let parent_frame = if page {
                    Some(text(info, "parentFrameId")?.to_owned())
                } else {
                    None
                };
                if self.sessions.len() >= CAPACITY
                    || self.sessions.contains_key(&Some(child.clone()))
                {
                    return Err("duplicate or excessive child session".into());
                }
                if let Some(parent) = &parent_frame {
                    identifier(parent)?;
                    if self
                        .frames
                        .get(parent)
                        .is_none_or(|frame| frame.session.as_deref() != session)
                    {
                        return Err(
                            "child session parent frame is absent or has another owner".into()
                        );
                    }
                }
                self.sessions.insert(
                    Some(child.clone()),
                    Session {
                        parent: event.session.clone(),
                        parent_frame,
                        page,
                        ready: false,
                        root_frame: None,
                    },
                );
                return Ok(Change::Attached {
                    session: child,
                    page,
                });
            }
            "Target.detachedFromTarget" => self.detach(session, text(params, "sessionId")?)?,
            "Page.frameAttached" => {
                let id = text(params, "frameId")?;
                let parent = text(params, "parentFrameId")?;
                identifier(id)?;
                identifier(parent)?;
                self.parent(id, Some(parent))?;
                if let Some(frame) = self.frames.get(id) {
                    if frame.parent.as_deref() != Some(parent) {
                        return Err("frame parent changed without navigation".into());
                    }
                } else {
                    self.capacity()?;
                    self.frames.insert(
                        id.into(),
                        Frame {
                            parent: Some(parent.into()),
                            session: event.session.clone(),
                            loader: String::new(),
                            url: String::new(),
                            context: None,
                        },
                    );
                }
            }
            "Page.frameNavigated" => self.frame(session, &params["frame"], None)?,
            "Page.frameDetached" => {
                let id = text(params, "frameId")?;
                identifier(id)?;
                if let Some(frame) = self.frames.get(id) {
                    let owns_parent = frame
                        .parent
                        .as_ref()
                        .and_then(|parent| self.frames.get(parent))
                        .is_some_and(|parent| parent.session.as_deref() == session);
                    if frame.session.as_deref() != session && !owns_parent {
                        return Err("frame detach has another session owner".into());
                    }
                }
                match text(params, "reason")? {
                    "swap" => self.invalidate(id, true),
                    "remove" => self.remove_frame(id),
                    _ => return Err("unsupported frame detach reason".into()),
                }
            }
            "Page.navigatedWithinDocument" => {
                let id = text(params, "frameId")?;
                let url = text(params, "url")?;
                let frame = self
                    .frames
                    .get_mut(id)
                    .ok_or("unknown same document frame")?;
                if frame.session.as_deref() != session {
                    return Err("same document navigation has another owner".into());
                }
                frame.url = url.into();
            }
            "Runtime.executionContextCreated" => return self.created(session, &params["context"]),
            "Runtime.executionContextDestroyed" => {
                let unique = params
                    .get("executionContextUniqueId")
                    .and_then(Value::as_str);
                let numeric = params.get("executionContextId").and_then(Value::as_i64);
                if unique.is_none() && numeric.is_none() {
                    return Err("missing destroyed context identity".into());
                }
                for frame in self
                    .frames
                    .values_mut()
                    .filter(|f| f.session.as_deref() == session)
                {
                    if frame.context.as_ref().is_some_and(|c| {
                        unique.map_or(numeric == Some(c.numeric), |u| u == c.unique)
                    }) {
                        frame.context = None;
                    }
                }
                self.provisional.retain(|(owner, _), context| {
                    owner.as_deref() != session
                        || !unique.map_or(numeric == Some(context.numeric), |u| u == context.unique)
                });
            }
            "Runtime.executionContextsCleared" => {
                for frame in self
                    .frames
                    .values_mut()
                    .filter(|f| f.session.as_deref() == session)
                {
                    frame.context = None;
                }
                self.provisional
                    .retain(|(owner, _), _| owner.as_deref() != session);
            }
            _ => (),
        }
        Ok(Change::None)
    }

    pub fn is_ready(&self) -> bool {
        self.main_context().is_some()
            && self.sessions.iter().all(|(key, session)| {
                !session.page
                    || session.ready
                        && session
                            .root_frame
                            .as_deref()
                            .and_then(|frame| self.context(frame))
                            .is_some_and(|context| context.session == *key)
            })
    }

    pub fn main_context(&self) -> Option<FrameContext> {
        self.context(self.main.as_deref()?)
    }

    pub fn context(&self, id: &str) -> Option<FrameContext> {
        let frame = self.frames.get(id)?;
        if !self.sessions.get(&frame.session)?.ready || frame.loader.is_empty() {
            return None;
        }
        Some(FrameContext {
            generation: self.generation,
            target: self.target.clone(),
            frame: id.into(),
            session: frame.session.clone(),
            unique: frame.context.as_ref()?.unique.clone(),
        })
    }

    pub fn contexts(&self) -> Vec<FrameContext> {
        let mut contexts: Vec<_> = self
            .frames
            .keys()
            .filter_map(|id| self.context(id))
            .collect();
        contexts.sort_by(|a, b| a.frame.cmp(&b.frame));
        contexts
    }

    pub fn validate(&self, context: &FrameContext) -> Result<()> {
        if self.context(context.frame_id()).as_ref() != Some(context) {
            return Err("browser execution context is stale".into());
        }
        Ok(())
    }

    pub fn parent_frame(&self, id: &str) -> Option<&str> {
        self.frames.get(id)?.parent.as_deref()
    }

    fn frame(&mut self, session: Option<&str>, value: &Value, parent: Option<&str>) -> Result<()> {
        self.session(session)?;
        let id = text(value, "id")?.to_owned();
        identifier(&id)?;
        let loader = text(value, "loaderId")?.to_owned();
        identifier(&loader)?;
        let url = text(value, "url")?.to_owned();
        let declared = value.get("parentId").and_then(Value::as_str);
        if declared.is_some() && parent.is_some() && declared != parent {
            return Err("frame tree parent mismatch".into());
        }
        let parent = declared
            .or(parent)
            .or(self.session(session)?.parent_frame.as_deref())
            .map(str::to_owned);
        self.parent(&id, parent.as_deref())?;
        let owner = session.map(str::to_owned);
        let existing = self.frames.get(&id);
        if existing.is_some_and(|f| f.session != owner && f.context.is_some()) {
            return Err("live frame has conflicting session ownership".into());
        }
        let changed = existing.is_some_and(|f| f.loader != loader || f.session != owner);
        let provisional = self.provisional.remove(&(owner.clone(), id.clone()));
        if changed {
            self.invalidate(&id, true);
        }
        let context = self
            .frames
            .get(&id)
            .and_then(|f| f.context.clone())
            .or(provisional);
        if !self.frames.contains_key(&id) {
            self.capacity()?;
        }
        if session.is_none() && parent.is_none() {
            if self.main.as_ref().is_some_and(|main| main != &id) {
                return Err("root frame identity changed".into());
            }
            self.main = Some(id.clone());
        }
        self.frames.insert(
            id,
            Frame {
                parent,
                session: owner,
                loader,
                url,
                context,
            },
        );
        Ok(())
    }

    fn created(&mut self, session: Option<&str>, value: &Value) -> Result<Change> {
        let numeric = value
            .get("id")
            .and_then(Value::as_i64)
            .ok_or("missing execution context number")?;
        let aux = value
            .get("auxData")
            .filter(|v| v.is_object())
            .ok_or("missing execution context metadata")?;
        let default = aux.get("isDefault").and_then(Value::as_bool);
        if default == Some(true) {
            let frame = text(aux, "frameId")?.to_owned();
            identifier(&frame)?;
            return Ok(Change::CreateWorld {
                session: session.map(str::to_owned),
                frame,
            });
        }
        if default != Some(false)
            || value.get("name").and_then(Value::as_str) != Some(AUTOMATION_WORLD)
        {
            return Ok(Change::None);
        }
        let frame = text(aux, "frameId")?.to_owned();
        identifier(&frame)?;
        let unique = text(value, "uniqueId")?.to_owned();
        identifier(&unique)?;
        let key = session.map(str::to_owned);
        if self
            .frames
            .iter()
            .any(|(id, f)| id != &frame && f.context.as_ref().is_some_and(|c| c.unique == unique))
            || self
                .provisional
                .iter()
                .any(|((owner, id), c)| (owner != &key || id != &frame) && c.unique == unique)
        {
            return Err("duplicate live unique execution context".into());
        }
        if let Some(current) = self.frames.get_mut(&frame).filter(|f| f.session == key) {
            if current.context.as_ref().is_some_and(|c| c.unique != unique) {
                current.context = None;
            }
            current.context = Some(Execution { numeric, unique });
        } else {
            if self.provisional.len() + self.frames.len() >= MAX_FRAMES {
                return Err("execution context capacity reached".into());
            }
            self.provisional
                .insert((key, frame), Execution { numeric, unique });
        }
        Ok(Change::None)
    }

    fn detach(&mut self, parent: Option<&str>, child: &str) -> Result<()> {
        let key = Some(child.to_owned());
        let session = self.sessions.get(&key).ok_or("unknown detached session")?;
        if session.parent.as_deref() != parent {
            return Err("detached session has another parent".into());
        }
        let mut removed = HashSet::from([key]);
        loop {
            let before = removed.len();
            for (key, session) in &self.sessions {
                if key.is_some() && removed.contains(&session.parent) {
                    removed.insert(key.clone());
                }
            }
            if before == removed.len() {
                break;
            }
        }
        let frames: Vec<_> = self
            .frames
            .iter()
            .filter(|(_, f)| removed.contains(&f.session))
            .map(|(id, _)| id.clone())
            .collect();
        for frame in frames {
            self.remove_frame(&frame);
        }
        self.sessions.retain(|key, _| !removed.contains(key));
        self.provisional
            .retain(|(key, _), _| !removed.contains(key));
        Ok(())
    }

    fn parent(&self, id: &str, parent: Option<&str>) -> Result<()> {
        let mut next = parent;
        let mut visited = HashSet::new();
        while let Some(current) = next {
            if current == id || !visited.insert(current) {
                return Err("browser frame parent cycle".into());
            }
            next = self
                .frames
                .get(current)
                .ok_or("unknown browser parent frame")?
                .parent
                .as_deref();
        }
        Ok(())
    }

    fn invalidate(&mut self, id: &str, retain: bool) {
        let mut removed = HashSet::from([id.to_owned()]);
        loop {
            let before = removed.len();
            for (id, frame) in &self.frames {
                if frame.parent.as_ref().is_some_and(|p| removed.contains(p)) {
                    removed.insert(id.clone());
                }
            }
            if before == removed.len() {
                break;
            }
        }
        self.frames
            .retain(|frame, _| !removed.contains(frame) || retain && frame == id);
        if let Some(frame) = self.frames.get_mut(id) {
            frame.context = None;
        }
        self.provisional
            .retain(|(_, frame), _| !removed.contains(frame));
    }

    fn remove_frame(&mut self, id: &str) {
        self.invalidate(id, false);
    }
    fn capacity(&self) -> Result<()> {
        if self.frames.len() + self.provisional.len() >= MAX_FRAMES {
            Err("browser frame capacity reached".into())
        } else {
            Ok(())
        }
    }
    fn session(&self, session: Option<&str>) -> Result<&Session> {
        self.sessions
            .get(&session.map(str::to_owned))
            .ok_or_else(|| "unknown browser session".into())
    }
    fn session_mut(&mut self, session: Option<&str>) -> Result<&mut Session> {
        self.sessions
            .get_mut(&session.map(str::to_owned))
            .ok_or_else(|| "unknown browser session".into())
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing browser lifecycle identity".into())
}
fn identifier(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        Err("invalid browser lifecycle identity".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use serde_json::json;

    fn event(session: Option<&str>, method: &'static str, params: Value) -> RawEvent {
        RawEvent {
            session: session.map(str::to_owned),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn child_context_identity_survives_only_its_own_generation_and_lifetime() {
        let mut map = ContextMap::new("selected-target").unwrap();
        map.tree(None,&json!({"frame":{"id":"root-frame","loaderId":"root-load","url":"https://example.test/"},
            "childFrames":[{"frame":{"id":"child-frame","parentId":"root-frame","loaderId":"placeholder","url":"https://child.test/"}}]})).unwrap();
        map.ready(None).unwrap();
        assert!(matches!(map.event(&event(None,"Target.attachedToTarget",json!({
            "sessionId":"actual-child-session","targetInfo":{"targetId":"different-child-target","type":"iframe","parentFrameId":"root-frame"}
        }))).unwrap(),Change::Attached {page:true,..}));
        assert!(
            matches!(map.event(&event(Some("actual-child-session"),"Runtime.executionContextCreated",json!({
            "context":{"id":6,"uniqueId":"page-context","auxData":{"frameId":"child-frame","isDefault":true}}
        }))).unwrap(),Change::CreateWorld{session:Some(owner),frame} if owner=="actual-child-session" && frame=="child-frame")
        );
        map.tree(Some("actual-child-session"),&json!({"frame":{"id":"child-frame","loaderId":"child-load","url":"https://child.test/"}})).unwrap();
        assert!(map.context("child-frame").is_none());
        map.ready(Some("actual-child-session")).unwrap();
        assert!(map.context("child-frame").is_none());
        map.event(&event(Some("actual-child-session"),"Runtime.executionContextCreated",json!({
            "context":{"id":8,"name":"other-extension","uniqueId":"foreign-context","auxData":{"frameId":"child-frame","isDefault":false}}
        }))).unwrap();
        assert!(map.context("child-frame").is_none());
        map.event(&event(Some("actual-child-session"),"Runtime.executionContextCreated",json!({
            "context":{"id":7,"name":AUTOMATION_WORLD,"uniqueId":"actual-unique-context","auxData":{"frameId":"child-frame","isDefault":false}}
        }))).unwrap();
        let context = map.context("child-frame").unwrap();
        assert_eq!(context.unique_context_id(), "actual-unique-context");
        assert_eq!(context.target_id(), "selected-target");
        assert_eq!(context.session_id(), "actual-child-session");
        assert_eq!(map.parent_frame("child-frame"), Some("root-frame"));
        map.validate(&context).unwrap();
        assert!(
            map.event(&event(
                Some("actual-child-session"),
                "Page.frameDetached",
                json!({"frameId":"root-frame","reason":"remove"})
            ))
            .is_err()
        );
        map.validate(&context).unwrap();
        assert!(
            ContextMap::new("selected-target")
                .unwrap()
                .validate(&context)
                .is_err()
        );
        map.event(&event(
            Some("actual-child-session"),
            "Runtime.executionContextsCleared",
            json!({}),
        ))
        .unwrap();
        assert!(map.validate(&context).is_err());
        map.event(&event(
            None,
            "Target.detachedFromTarget",
            json!({"sessionId":"actual-child-session"}),
        ))
        .unwrap();
        assert!(map.context("child-frame").is_none());
        assert!(
            map.event(&event(
                Some("actual-child-session"),
                "Runtime.executionContextsCleared",
                json!({})
            ))
            .is_err()
        );
    }
}
