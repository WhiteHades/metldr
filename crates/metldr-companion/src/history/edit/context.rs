use super::{Edit, record, storage};
use crate::{
    Result,
    agent_model::Launch,
    history::{context::Route, edit::Selection, schema::*},
    model::{Run, Status},
    provider_settings::Protocol,
};
use redb::ReadableTable;

impl Edit<'_> {
    pub(crate) fn fixture_context(&self, expected: &Selection, source: &str) -> Result<Vec<Run>> {
        self.completed_context(expected, source, Route::Fixture, 16)
    }

    pub(crate) fn text_context(
        &self,
        expected: &Selection,
        source: &str,
        protocol: Protocol,
        fingerprint: &str,
    ) -> Result<Vec<Run>> {
        self.completed_context(expected, source, Route::Text(protocol, fingerprint), 6)
    }

    fn completed_context(
        &self,
        expected: &Selection,
        source: &str,
        route: Route<'_>,
        limit: usize,
    ) -> Result<Vec<Run>> {
        let result = (|| {
            self.require_selection(expected)?;
            storage(|| {
                let key = route.key(&expected.conversation, source)?;
                let table = self.write.open_table(CONTEXT)?;
                let mut values = Vec::new();
                for entry in table
                    .range((key.as_str(), 0)..=(key.as_str(), u64::MAX))?
                    .rev()
                    .take(limit)
                {
                    let (order, id) = entry?;
                    let run = self.saved_run(id.value())?.ok_or("context run is absent")?;
                    let launch: Record<Launch> = record(self.write, LAUNCHES, id.value())?
                        .ok_or("context launch is absent")?;
                    let number = run
                        .value
                        .id
                        .rsplit(':')
                        .next()
                        .ok_or("invalid context identity")?
                        .parse::<u64>()?;
                    if run.conversation != expected.conversation
                        || launch.conversation != run.conversation
                        || run.value.source != source
                        || run.value.status != Status::Complete
                        || order.value().1 != number
                        || Route::from_launch(&launch.value)
                            .map(|route| route.key(&run.conversation, source))
                            .transpose()?
                            .as_deref()
                            != Some(key.as_str())
                    {
                        return Err("completed context index differs".into());
                    }
                    self.validate_flat(&run.value, &launch.value, self.source(source)?)?;
                    values.push(run.value);
                }
                values.reverse();
                Ok(values)
            })
        })();
        self.checked(result)
    }
}
