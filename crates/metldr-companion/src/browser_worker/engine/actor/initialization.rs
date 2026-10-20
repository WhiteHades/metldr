use super::*;
impl Actor {
    pub(super) fn initialize(&mut self, session: Option<String>, page: bool) -> Result<()> {
        self.initialize_step(Initialization {
            session,
            page,
            step: 0,
            deadline: if self.ready.is_some() {
                self.init_deadline
            } else {
                Instant::now() + TIMEOUT
            },
        })
    }
    pub(super) fn initialize_step(&mut self, init: Initialization) -> Result<()> {
        let command = if !init.page {
            (init.step == 0).then(|| ("Runtime.runIfWaitingForDebugger", json!({})))
        } else {
            match init.step {
                0 => Some(("Page.enable", json!({}))),
                1 => Some(("Page.getFrameTree", json!({}))),
                2 => Some(("Runtime.enable", json!({}))),
                3 => Some(("Page.setLifecycleEventsEnabled", json!({"enabled":true}))),
                4 => Some(("Network.enable", json!({}))),
                5 => Some(("Network.setCacheDisabled", json!({"cacheDisabled":false}))),
                6 => Some((
                    "Target.setAutoAttach",
                    json!({"autoAttach":true,"flatten":true,"waitForDebuggerOnStart":false}),
                )),
                7 => Some(("Performance.enable", json!({}))),
                8 => Some(("Log.enable", json!({}))),
                9 => Some(("Runtime.runIfWaitingForDebugger", json!({}))),
                _ => None,
            }
        };
        if let Some((method, params)) = command {
            self.queue(
                method,
                init.session.clone(),
                params,
                Work::Init(init),
                None,
                None,
            )
        } else {
            self.contexts.ready(init.session.as_deref())?;
            Ok(())
        }
    }
}
