//! The sessions a console lists: what an operator calls a conversation of
//! theirs with her, and which of them they have set aside. Both go in her
//! journal, so that every console, now or later, lists the same ones.

use ghost_core::{journal::Event, social};
use tracing::info;

use super::{Mind, Operator, SessionChange};
use crate::Result;

impl Mind {
    /// The operator `by` names a session or sets it aside. Returns whether
    /// it was done: only a session that is listed is set aside.
    pub(super) fn on_session(&mut self, change: SessionChange, by: &Operator) -> Result<bool> {
        match change {
            SessionChange::Name { place, title } => {
                info!("{place}: {} calls it {title:?}", by.key);
                self.record(Event::SessionNamed {
                    place,
                    by: by.key.clone(),
                    title,
                })?;
            }
            SessionChange::SetAside { place } => {
                let channel = social::split_key(&place).map_or("", |(channel, _)| channel);
                let listed = self
                    .ghost
                    .memory()
                    .sessions(channel)?
                    .iter()
                    .any(|s| s.key == place && !s.set_aside);
                if !listed {
                    return Ok(false);
                }
                info!("{place}: {} sets it aside", by.key);
                self.record(Event::SessionSetAside {
                    place,
                    by: by.key.clone(),
                })?;
            }
        }
        Ok(true)
    }
}
