mod config;
mod installation;
pub(super) use config::Fixture;

#[cfg(test)]
pub(super) use crate::wire::read_frame;
#[cfg(test)]
pub(super) use config::{parse_bootstrap_config, valid_parent_window_arg};

use crate::{
    Result,
    flow::{Event, Owner},
    store::Disk,
    transport::{Action, Transport},
};
use config::{Config, Mode};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::Ordering,
        mpsc::{self, RecvTimeoutError},
    },
    time::Duration,
};

pub(super) fn run() -> Result<()> {
    let config = Config::load()?;
    if let Mode::Bridge = config.mode {
        let bridge = config
            .bridge
            .as_ref()
            .ok_or("bridge configuration absent")?;
        return crate::transport::bridge(
            &bridge.socket,
            &bridge.capability,
            &config.extension_origin,
        );
    }
    let (disk, mut state) = Disk::open(config.state.clone())?;
    let fixture_profiles = (
        if matches!(
            state
                .agent_settings
                .slot(crate::agent_settings::Kind::Pi)
                .selection,
            crate::agent_settings::Selection::Unmanaged
        ) {
            config.profile()?
        } else {
            None
        },
        if matches!(
            state
                .agent_settings
                .slot(crate::agent_settings::Kind::Codex)
                .selection,
            crate::agent_settings::Selection::Unmanaged
        ) {
            config.codex_profile()?
        } else {
            None
        },
        if matches!(
            state
                .agent_settings
                .slot(crate::agent_settings::Kind::Opencode)
                .selection,
            crate::agent_settings::Selection::Unmanaged
        ) {
            config.opencode_profile()?
        } else {
            None
        },
    );
    let transport = match config.mode {
        Mode::Stdio => Transport::stdio(),
        Mode::SocketOwner => {
            let bridge = config.bridge.ok_or("owner bridge configuration absent")?;
            Transport::socket(bridge.socket, bridge.capability, config.extension_origin)?
        }
        Mode::Bridge => unreachable!(),
    };
    use crate::provider_settings::Protocol;
    let browser_route = if state.providers.managed(Protocol::Messages) {
        None
    } else {
        config
            .browser_route
            .map(|r| r.bind(config.fixture.as_ref().map(|fixture| fixture.address)))
            .transpose()?
    };
    let responses_route = if state.providers.managed(Protocol::Responses) {
        None
    } else {
        config
            .responses_route
            .map(|r| r.bind_responses(config.fixture.as_ref().map(|fixture| fixture.address)))
            .transpose()?
    };
    let google_route = if state.providers.managed(Protocol::GoogleInteractions) {
        None
    } else {
        config
            .google_route
            .map(|r| r.bind_google(config.fixture.as_ref().map(|fixture| fixture.address)))
            .transpose()?
    };
    let chat_route = if state.providers.managed(Protocol::ChatCompletions) {
        None
    } else {
        config
            .chat_route
            .map(|r| {
                r.bind_chat_completions(config.fixture.as_ref().map(|fixture| fixture.address))
            })
            .transpose()?
    };
    if let Some(pending) = state.providers.pending.take() {
        state.providers.retired.push(pending.connection.credential);
    }
    crate::recovery::interrupt(&mut state)?;
    let (tx, rx) = mpsc::sync_channel(64);
    let mut owner = Owner {
        disk,
        settings: Default::default(),
        agent_runtime: Default::default(),
        state,
        fixture: config.fixture,
        subscribers: BTreeMap::new(),
        operation: None,
        tx,
        pi_profile: fixture_profiles.0,
        codex_profile: fixture_profiles.1,
        opencode_profile: fixture_profiles.2,
        native_permissions: BTreeMap::new(),
        pi_ui: None,
        browser_approval: None,
        browser_io: None,
        browser_route,
        responses_route,
        google_route,
        chat_route,
        transport,
    };
    owner.preflight(&owner.state)?;
    owner.disk.save(&owner.state)?;
    owner.transport.start(owner.tx.clone())?;
    for kind in [
        crate::agent_settings::Kind::Pi,
        crate::agent_settings::Kind::Codex,
        crate::agent_settings::Kind::Opencode,
    ] {
        owner.refresh_agent_runtime(kind);
    }
    if owner.state.providers.slots.values().any(|s| {
        matches!(
            s.selection,
            crate::provider_settings::Selection::Configured { .. }
        )
    }) || !owner.state.providers.retired.is_empty()
    {
        owner.provider_load(None)?;
    }
    owner.reply_view(None, "ready", true)?;
    let mut closed = false;
    let mut input_error = None;
    loop {
        owner.settings.check_deadline()?;
        if closed && owner.operation.is_none() && owner.settings.job.is_none() {
            break;
        }
        if !closed {
            owner.transport.poll(&owner.tx)?;
        }
        let event = match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(error) => return Err(error.into()),
        };
        match event {
            Event::Provider(completion) => owner.provider_completed(completion)?,
            Event::BrowserIo(call) => owner.browser_io_call(call)?,
            Event::Browser(event) => owner.browser_event(event)?,
            Event::Input(input) => match owner.transport.input(input)? {
                Action::Connected => {
                    owner.subscribers.clear();
                    if !closed {
                        owner.reply_view(None, "ready", true)?;
                    }
                }
                Action::Request(request) if !closed => owner.request(request)?,
                Action::Detached => {
                    owner.subscribers.clear();
                    owner.disconnect_pi_ui()?;
                    if let Some(run) = owner.browser_io.as_ref().map(|s| s.run.clone()) {
                        owner.browser_io_finish(&run);
                        owner.cancel_browser_approval(&run);
                        owner.cancel_native_permissions(&run);
                    }
                }
                Action::Shutdown(error) => {
                    owner.disconnect_pi_ui()?;
                    if let Some(run) = owner.browser_io.as_ref().map(|s| s.run.clone()) {
                        owner.browser_io_finish(&run);
                    }
                    closed = true;
                    input_error = error;
                    if let Some(operation) = &owner.operation {
                        operation.stop.store(true, Ordering::Release);
                    }
                    if let Some(run) = owner.operation.as_ref().map(|op| op.run.clone()) {
                        owner.cancel_browser_approval(&run);
                        owner.cancel_native_permissions(&run);
                    }
                }
                _ => (),
            },
            Event::Text { run, change } => owner.text(&run, change)?,
            Event::Settled { run, result } => owner.settled(&run, result, closed)?,
            Event::PiBound {
                run,
                workflow,
                binding,
                ack,
            } => owner.pi_bound(&run, &workflow, binding, ack)?,
            Event::ToolProgress { run, call } => owner.tool_progress(&run, call)?,
            Event::PiActivity { run, activity } => owner.pi_activity(&run, activity)?,
            Event::PiUi { run, change } => owner.pi_ui_change(&run, change)?,
            Event::PiSettled {
                run,
                result,
                released,
            } => owner.agent_settled(&run, result, closed, released)?,
            Event::CodexBound {
                run,
                workflow,
                identity,
                selection,
                ack,
            } => owner.codex_bound(&run, &workflow, identity, selection, ack)?,
            Event::CodexActivity { run, activity } => owner.codex_activity(&run, activity)?,
            Event::CodexSettled {
                run,
                result,
                released,
            } => owner.agent_settled(&run, result, closed, released)?,
            Event::OpenCodeBound {
                run,
                workflow,
                identity,
                ack,
            } => owner.opencode_bound(&run, &workflow, identity, ack)?,
            Event::NativePermission {
                run,
                key,
                details,
                ack,
            } => owner.native_permission(&run, key, details, ack)?,
            Event::NativePermissionResolved { run, key } => {
                owner.native_permission_resolved(&run, key)?
            }
            Event::OpenCodeActivity { run, activity } => owner.opencode_activity(&run, activity)?,
            Event::OpenCodeSettled {
                run,
                result,
                released,
            } => owner.agent_settled(&run, result, closed, released)?,
        }
    }
    drop(rx);
    owner.transport.finish()?;
    if let Some(error) = input_error {
        return Err(error.into());
    }
    Ok(())
}
