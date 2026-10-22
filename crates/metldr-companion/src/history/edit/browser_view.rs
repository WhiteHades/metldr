use super::{Edit, Selection, storage};
use crate::{Result, browser_action::Action, history::schema::*};
use redb::ReadableTable;
use std::collections::BTreeMap;

impl Edit<'_> {
    pub(crate) fn browser_view(
        &self,
        scope: &Selection,
        runs: &[&str],
    ) -> Result<BTreeMap<String, Action>> {
        self.checked((|| {
            self.require_selection(scope)?;
            if runs.len() > 2 {
                return Err("browser projection exceeds its run bound".into());
            }
            let mut chosen = BTreeMap::new();
            for run in runs {
                self.run(scope, run)?;
                let ids = storage(|| {
                    self.write
                        .open_table(ACTION_ORDER)?
                        .range((*run, 0, "")..=(*run, u64::MAX, "~"))?
                        .rev()
                        .take(16)
                        .map(|entry| {
                            let (key, id) = entry?;
                            Ok((
                                key.value().1,
                                key.value().2.to_owned(),
                                id.value().to_owned(),
                            ))
                        })
                        .collect::<Result<Vec<_>>>()
                })?;
                for (number, key, id) in ids {
                    let action = self.browser_action(scope, &id)?;
                    if key != id
                        || action.proposal.run != *run
                        || id.rsplit(':').next().unwrap().parse::<u64>()? != number
                    {
                        return storage(|| Err("browser projection order differs".into()));
                    }
                    chosen.insert((number, id.clone()), action);
                }
            }
            while chosen.len() > 16 {
                chosen.pop_first();
            }
            for run in runs {
                if let Some(action) = self.browser_open(scope, run)? {
                    let number = action.id.rsplit(':').next().unwrap().parse::<u64>()?;
                    if !chosen.contains_key(&(number, action.id.clone())) {
                        if chosen.len() == 16 {
                            chosen.pop_first();
                        }
                        chosen.insert((number, action.id.clone()), action);
                    }
                }
            }
            Ok(chosen.into_values().map(|a| (a.id.clone(), a)).collect())
        })())
    }
}
