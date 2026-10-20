use super::*;

impl Tools {
    pub(super) async fn navigate(
        &mut self,
        tool_use_id: &str,
        destination: &str,
        stop: &AtomicBool,
    ) -> Result<Value> {
        if !crate::browser_action::valid_destination(&self.current.url, destination) {
            return Err("browser navigation URL is outside the owned fixture".into());
        }
        let proposal = self
            .engine
            .as_ref()
            .ok_or("browser is not attached")?
            .prepare_navigation(tool_use_id, destination)
            .await?;
        let approved = action(&self.client, stop, |ack| browser_owner::Event::Proposed {
            proposal,
            ack,
        })
        .await?;
        let settled = match &approved.state {
            Phase::Approved => {
                let dispatched = action(&self.client, stop, |ack| browser_owner::Event::Dispatch {
                    action: approved.id.clone(),
                    revision: approved.revision,
                    ack,
                })
                .await?;
                let outcome = match self
                    .engine
                    .as_ref()
                    .ok_or("browser is not attached")?
                    .navigate(&dispatched)
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(_) => Outcome::Uncertain {
                        reason: "navigation destination or effect could not be verified".into(),
                    },
                };
                action_observed(&self.client, &dispatched, outcome).await?
            }
            Phase::Settled { .. } => approved,
            _ => return Err("browser navigation was not approved or settled".into()),
        };
        match settled.state {
            Phase::Settled {
                outcome: Outcome::Navigated { binding, url, .. },
            } => {
                self.current.document = binding.document;
                self.current.url = url;
                self.observe().await
            }
            Phase::Settled {
                outcome: outcome @ Outcome::Prevented { .. },
            } => Ok(serde_json::to_value(outcome)?),
            _ => Err("navigation effect is uncertain; automatic continuation stopped".into()),
        }
    }
}
