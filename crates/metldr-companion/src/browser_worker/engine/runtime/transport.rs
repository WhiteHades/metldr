use super::{CAPACITY, MAX_MESSAGE, RawEvent, Result, TIMEOUT, WireCommand, socket::Socket};
use chromiumoxide::{
    cdp::browser_protocol::target::SessionId,
    types::{CallId, Message, MethodId},
};
use futures::StreamExt;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use tokio::time::{Instant, sleep_until};

/// The caller owns context selection, approvals and actual Chrome dispatch.
/// This transport correlates the normal Chromiumoxide connection with that caller.
pub struct Transport {
    socket: Socket,
    pending: HashMap<CallId, Pending>,
    commands: VecDeque<WireCommand>,
    undecoded_events: usize,
    admitted: usize,
    ended: bool,
}

pub enum Output {
    Dispatch(Dispatch),
    Event(RawEvent),
    Reply { id: CallId, result: Result<Value> },
}

pub struct Dispatch {
    pub id: CallId,
    pub method: MethodId,
    pub session: Option<String>,
    pub params: Value,
}

struct Pending {
    command: WireCommand,
    deadline: Instant,
    state: State,
}

#[derive(PartialEq)]
enum State {
    Queued,
    Offered,
    Dispatched,
    Replied,
}

impl Transport {
    pub async fn connect(deadline: Instant) -> Result<Self> {
        Ok(Self {
            socket: Socket::open(deadline).await?,
            pending: HashMap::new(),
            commands: VecDeque::new(),
            undecoded_events: 0,
            admitted: 0,
            ended: false,
        })
    }

    pub fn submit(
        &mut self,
        method: MethodId,
        session: Option<String>,
        params: Value,
    ) -> Result<CallId> {
        self.live()?;
        if self.pending.len() >= CAPACITY || self.admitted == 1_000_000 {
            return Err("browser command capacity reached".into());
        }
        let candidate = WireCommand {
            id: CallId::new(0),
            method,
            session,
            params,
        };
        candidate.validate()?;
        let encoded = serde_json::to_vec(&candidate).map_err(|_| "invalid browser command")?;
        // Reserve space for the largest permitted connection ID.
        if encoded.len() > MAX_MESSAGE - 20 {
            return Err("browser command exceeds message limit".into());
        }
        let id = self
            .socket
            .connection
            .submit_command(
                candidate.method.clone(),
                candidate.session.clone().map(SessionId::new),
                candidate.params.clone(),
            )
            .map_err(|_| "cannot queue browser command")?;
        self.pending.insert(
            id,
            Pending {
                command: WireCommand { id, ..candidate },
                deadline: Instant::now() + TIMEOUT,
                state: State::Queued,
            },
        );
        self.admitted += 1;
        Ok(id)
    }

    /// Queue notices before polling dispatch, in the order received from Chrome.
    pub fn event(&mut self, event: RawEvent) -> Result<()> {
        self.live()?;
        if event.validate().is_err() {
            return self.fail("invalid browser event");
        }
        if self.undecoded_events == CAPACITY {
            return self.fail("browser event decode capacity reached");
        }
        let value = serde_json::to_value(event).map_err(|_| "invalid browser event")?;
        self.send(value)?;
        self.undecoded_events += 1;
        Ok(())
    }

    pub fn dispatch(&mut self, command: &Dispatch) -> Result<()> {
        self.live()?;
        let Some(pending) = self.pending.get(&command.id) else {
            return self.fail("unknown browser dispatch");
        };
        if pending.deadline <= Instant::now() {
            return self.fail("browser dispatch deadline expired");
        }
        if self.undecoded_events != 0 {
            return Err("browser notices must be decoded before dispatch".into());
        }
        if pending.state != State::Offered
            || pending.command.method != command.method
            || pending.command.session != command.session
            || pending.command.params != command.params
        {
            return self.fail("browser dispatch does not match admitted request");
        }
        self.pending.get_mut(&command.id).unwrap().state = State::Dispatched;
        Ok(())
    }

    pub fn reject(&mut self, id: CallId) -> Result<()> {
        self.live()?;
        let Some(pending) = self.pending.get(&id) else {
            return self.fail("unknown rejected browser request");
        };
        if pending.state != State::Offered || pending.deadline <= Instant::now() {
            return self.fail("rejected browser request is stale or already dispatched");
        }
        self.send(json!({"id":id,"error":{"code":-32001,"message":"browser command prevented"}}))?;
        self.pending.get_mut(&id).unwrap().state = State::Replied;
        Ok(())
    }

    pub fn reply(&mut self, id: CallId, result: Result<Value>) -> Result<()> {
        self.live()?;
        let Some(pending) = self.pending.get(&id) else {
            return self.fail("unknown browser reply");
        };
        if pending.deadline <= Instant::now() {
            return self.fail("browser reply deadline expired");
        }
        if pending.state != State::Dispatched {
            return self.fail("browser reply was not dispatched or was already supplied");
        }
        let value = match result {
            Ok(value) if value.is_object() => json!({"id":id,"result":value}),
            Ok(_) => return self.fail("browser result must be an object"),
            Err(_) => json!({"id":id,"error":{"code":-32000,"message":"browser command failed"}}),
        };
        self.send(value)?;
        self.pending.get_mut(&id).unwrap().state = State::Replied;
        Ok(())
    }

    pub fn events_pending(&self) -> bool {
        self.undecoded_events != 0
    }

    pub async fn next(&mut self) -> Result<Output> {
        self.live()?;
        loop {
            let deadline = self.pending.values().map(|p| p.deadline).min();
            if deadline.is_some_and(|d| d <= Instant::now()) {
                return self.fail("browser command deadline expired");
            }
            if self.undecoded_events == 0
                && let Some(command) = self.commands.pop_front()
            {
                let pending = self
                    .pending
                    .get_mut(&command.id)
                    .ok_or("unknown browser command")?;
                if pending.state != State::Queued || pending.command != command {
                    return self.fail("browser command does not match admitted request");
                }
                pending.state = State::Offered;
                return Ok(Output::Dispatch(Dispatch {
                    id: command.id,
                    method: command.method,
                    session: command.session,
                    params: command.params,
                }));
            }
            // Idle transport has no request deadline. Socket failure still ends the stream.
            let sleep = sleep_until(deadline.unwrap_or_else(|| Instant::now() + TIMEOUT));
            tokio::select! {
                biased;
                message = self.socket.connection.next() => {
                    match message {
                        Some(Ok(Message::Event(event))) => {
                            if event.validate().is_err() { return self.fail("invalid browser socket event"); }
                            if self.undecoded_events == 0 {
                                return self.fail("unsolicited browser socket event");
                            }
                            self.undecoded_events -= 1;
                            return Ok(Output::Event(event));
                        }
                        Some(Ok(Message::Response(response))) => {
                            let Some(pending) = self.pending.remove(&response.id) else { return self.fail("unknown browser response"); };
                            if pending.deadline<=Instant::now() { return self.fail("browser response deadline expired") }
                            if pending.state != State::Replied {
                                return self.fail("browser response preceded dispatch");
                            }
                            let result = match (response.result, response.error) {
                                (Some(value), None) if value.is_object() => Ok(value),
                                (None, Some(_)) => Err("browser command failed".into()),
                                _ => return self.fail("invalid browser response"),
                            };
                            return Ok(Output::Reply { id:response.id, result });
                        }
                        Some(Err(_)) | None => return self.fail("browser connection ended"),
                    }
                }
                command = self.socket.commands.recv(), if self.commands.len() < CAPACITY => {
                    let Some(command) = command else { return self.fail("browser socket ended") };
                    self.commands.push_back(command);
                }
                _ = sleep => {
                    if deadline.is_some() { return self.fail("browser command deadline expired") }
                }
            }
        }
    }

    pub async fn close(mut self) -> Result<()> {
        self.ended = true;
        self.pending.clear();
        self.commands.clear();
        self.socket.shutdown().await
    }

    fn live(&self) -> Result<()> {
        if self.ended {
            Err("browser transport ended".into())
        } else {
            Ok(())
        }
    }

    fn send(&mut self, value: Value) -> Result<()> {
        if serde_json::to_vec(&value)
            .map_err(|_| "invalid browser message")?
            .len()
            > MAX_MESSAGE
        {
            return self.fail("browser message exceeds limit");
        }
        if self.socket.incoming.try_send(value).is_err() {
            return self.fail("browser incoming queue unavailable or full");
        }
        Ok(())
    }

    fn fail<T>(&mut self, message: &str) -> Result<T> {
        self.ended = true;
        self.socket.abort();
        Err(message.into())
    }
}

#[cfg(test)]
mod checks {
    use super::*;

    #[tokio::test]
    async fn normal_connection_preserves_events_before_dispatch_and_rejects_duplicate_reply() {
        let mut transport = Transport::connect(Instant::now() + TIMEOUT).await.unwrap();
        let id = transport
            .submit(
                "Runtime.enable".into(),
                Some("child-session".into()),
                json!({}),
            )
            .unwrap();
        let event = RawEvent {
            method: "Runtime.bindingCalled".into(),
            session: Some("child-session".into()),
            params: json!({"futureEnum":"unknown","payload":{"text":"kept"}}),
        };
        transport.event(event.clone()).unwrap();
        match transport.next().await.unwrap() {
            Output::Event(actual) => assert_eq!(actual, event),
            _ => panic!("dispatch overtook a queued lifecycle notice"),
        }
        match transport.next().await.unwrap() {
            Output::Dispatch(command) => {
                assert_eq!(command.id, id);
                assert_eq!(command.session.as_deref(), Some("child-session"));
                assert_eq!(command.method.as_ref(), "Runtime.enable");
                transport.dispatch(&command).unwrap();
            }
            _ => panic!("admitted command did not reach dispatch"),
        }
        transport.reply(id, Ok(json!({}))).unwrap();
        match transport.next().await.unwrap() {
            Output::Reply { id: reply, result } => {
                assert_eq!(reply, id);
                assert_eq!(result.unwrap(), json!({}));
            }
            _ => panic!("normal Connection did not decode the response"),
        }
        assert!(transport.reply(id, Ok(json!({}))).is_err());
        transport.close().await.unwrap();
    }
}

#[cfg(test)]
mod deadline_check {
    use super::*;

    #[tokio::test]
    async fn late_reply_ends_the_connection_without_another_poll() {
        let mut transport = Transport::connect(Instant::now() + TIMEOUT).await.unwrap();
        let id = transport
            .submit("Page.enable".into(), None, json!({}))
            .unwrap();
        let Output::Dispatch(command) = transport.next().await.unwrap() else {
            panic!("command was not offered")
        };
        transport.dispatch(&command).unwrap();
        tokio::time::sleep(TIMEOUT + std::time::Duration::from_millis(10)).await;
        assert_eq!(
            transport.reply(id, Ok(json!({}))).unwrap_err(),
            "browser reply deadline expired"
        );
        assert!(
            transport
                .submit("Page.enable".into(), None, json!({}))
                .is_err()
        );
        transport.close().await.unwrap();
    }
}
