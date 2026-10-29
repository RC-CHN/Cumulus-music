//! Ephemeral native registration results. Persisted settings never imply active
//! hooks, and an old native callback cannot acknowledge a replacement plan.
use super::{Action, Chord, Scope, ShortcutSettings};
use crate::Result;
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "action", rename_all = "snake_case")]
pub enum Hotkey {
    Configured(Action),
    F5,
    Space,
    Close,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    MediaStop,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Registration {
    pub hotkey: Hotkey,
    pub scope: Scope,
    pub chord: Chord,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegistrationStatus {
    Pending,
    Active,
    Conflict { code: i32 },
    Unavailable { code: i32 },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket(uuid::Uuid);
impl Ticket {
    /// Public registration generation, not an account or media capability.
    pub fn id(&self) -> String {
        self.0.to_string()
    }
}
pub struct RegistrationLedger {
    ticket: Ticket,
    rows: BTreeMap<(Hotkey, Scope), (Registration, RegistrationStatus)>,
}
impl ShortcutSettings {
    pub fn registration_plan(&self) -> Result<Vec<Registration>> {
        self.validate()?;
        let mut plan = vec![];
        for (&action, b) in &self.bindings {
            for (scope, chord) in [(Scope::Local, &b.local), (Scope::Global, &b.global)] {
                if (scope == Scope::Local || self.global_enabled)
                    && let Some(chord) = chord
                {
                    plan.push(Registration {
                        hotkey: Hotkey::Configured(action),
                        scope,
                        chord: chord.clone(),
                    });
                }
            }
        }
        for (hotkey, keys) in [
            (Hotkey::F5, vec![116]),
            (Hotkey::Space, vec![32]),
            (Hotkey::Close, vec![18, 115]),
        ] {
            plan.push(Registration {
                hotkey,
                scope: Scope::Local,
                chord: Chord::fixed(&keys),
            });
        }
        if self.media_enabled {
            for (hotkey, key) in [
                (Hotkey::MediaPlayPause, 179),
                (Hotkey::MediaNext, 176),
                (Hotkey::MediaPrevious, 177),
                (Hotkey::MediaStop, 178),
            ] {
                plan.push(Registration {
                    hotkey,
                    scope: Scope::Global,
                    chord: Chord::fixed(&[key]),
                });
            }
        }
        Ok(plan)
    }
}
impl RegistrationLedger {
    pub fn new(settings: &ShortcutSettings) -> Result<Self> {
        Ok(Self {
            ticket: Ticket(uuid::Uuid::new_v4()),
            rows: settings
                .registration_plan()?
                .into_iter()
                .map(|r| ((r.hotkey, r.scope), (r, RegistrationStatus::Pending)))
                .collect(),
        })
    }
    pub fn ticket(&self) -> Ticket {
        self.ticket.clone()
    }
    pub fn rows(&self) -> impl Iterator<Item = (&Registration, RegistrationStatus)> {
        self.rows.values().map(|(r, s)| (r, *s))
    }
    pub fn replace(&mut self, settings: &ShortcutSettings) -> Result<()> {
        // Validate first: an invalid replacement must not retire a working plan.
        *self = Self::new(settings)?;
        Ok(())
    }
    pub fn acknowledge(
        &mut self,
        ticket: &Ticket,
        hotkey: Hotkey,
        scope: Scope,
        status: RegistrationStatus,
    ) -> bool {
        if &self.ticket != ticket {
            return false;
        }
        let Some((_, state)) = self.rows.get_mut(&(hotkey, scope)) else {
            return false;
        };
        *state = status;
        true
    }
    pub fn active(&self, ticket: &Ticket, hotkey: Hotkey, scope: Scope) -> bool {
        &self.ticket == ticket
            && self
                .rows
                .get(&(hotkey, scope))
                .is_some_and(|(_, s)| *s == RegistrationStatus::Active)
    }
}
