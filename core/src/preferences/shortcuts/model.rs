use super::{Chord, damaged};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    PlayPause,
    #[serde(rename = "prev")]
    Previous,
    Next,
    #[serde(rename = "add_volume")]
    VolumeUp,
    #[serde(rename = "cut_volume")]
    VolumeDown,
    #[serde(rename = "mini_max")]
    MiniMode,
    #[serde(rename = "love")]
    Like,
    #[serde(rename = "lyric")]
    DesktopLyrics,
    Translate,
}
const DEFAULTS: &[(Action, u16)] = &[
    (Action::PlayPause, 80),
    (Action::Previous, 37),
    (Action::Next, 39),
    (Action::VolumeUp, 38),
    (Action::VolumeDown, 40),
    (Action::MiniMode, 77),
    (Action::Like, 76),
    (Action::DesktopLyrics, 68),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Local,
    Global,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    #[serde(deserialize_with = "required_chord")]
    pub local: Option<Chord>,
    #[serde(deserialize_with = "required_chord")]
    pub global: Option<Chord>,
}
fn required_chord<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Chord>, D::Error> {
    Option::deserialize(d)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutSettings {
    pub revision: String,
    pub global_enabled: bool,
    pub media_enabled: bool,
    pub bindings: BTreeMap<Action, Binding>,
}
impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            global_enabled: true,
            media_enabled: true,
            bindings: DEFAULTS
                .iter()
                .map(|&(action, key)| {
                    (
                        action,
                        Binding {
                            local: Some(Chord::fixed(&[17, key])),
                            global: Some(Chord::fixed(&[17, 18, key])),
                        },
                    )
                })
                .collect(),
        }
    }
}
impl ShortcutSettings {
    /// M3097 resets its eight default rows and both switches, leaving an
    /// already-present gated translation row intact. Reject any collision
    /// atomically instead of saving a map with two interpretations of one key.
    pub fn reset_defaults(&mut self) -> Result<()> {
        let mut next = Self {
            revision: self.revision.clone(),
            ..Self::default()
        };
        if let Some(binding) = self.bindings.get(&Action::Translate) {
            next.bindings.insert(Action::Translate, binding.clone());
        }
        next.validate()
            .map_err(|_| Error::State("shortcut conflict"))?;
        *self = next;
        Ok(())
    }
    pub fn bind(&mut self, action: Action, scope: Scope, chord: Option<Chord>) -> Result<()> {
        if !self.bindings.contains_key(&action) {
            return Err(Error::InvalidInput("shortcut action unavailable"));
        }
        if let Some(chord) = &chord {
            chord.validate_custom()?;
            for (&other, entry) in &self.bindings {
                if (entry.local.as_ref() == Some(chord)
                    && (other != action || scope != Scope::Local))
                    || (entry.global.as_ref() == Some(chord)
                        && (other != action || scope != Scope::Global))
                {
                    return Err(Error::State("shortcut conflict"));
                }
            }
        }
        let entry = self.bindings.get_mut(&action).expect("checked action");
        match scope {
            Scope::Local => entry.local = chord,
            Scope::Global => entry.global = chord,
        }
        Ok(())
    }
    /// Reflect an observed translation feature gate, never manufacture one.
    /// Official M147 leaves the default empty if Ctrl+T is already occupied.
    pub fn translation_gate(&mut self, enabled: bool) {
        if !enabled {
            self.bindings.remove(&Action::Translate);
            return;
        }
        if self.bindings.contains_key(&Action::Translate) {
            return;
        }
        let candidate = Chord::fixed(&[17, 84]);
        let conflict = self
            .bindings
            .values()
            .any(|b| b.local.as_ref() == Some(&candidate) || b.global.as_ref() == Some(&candidate));
        self.bindings.insert(
            Action::Translate,
            Binding {
                local: (!conflict).then_some(candidate),
                global: None,
            },
        );
    }
    pub(super) fn validate(&self) -> Result<()> {
        if !DEFAULTS.iter().all(|(a, _)| self.bindings.contains_key(a)) {
            return Err(damaged());
        }
        let mut seen = BTreeSet::new();
        for b in self.bindings.values() {
            for chord in [&b.local, &b.global].into_iter().flatten() {
                chord.validate_custom().map_err(|_| damaged())?;
                if !seen.insert(chord) {
                    return Err(damaged());
                }
            }
        }
        Ok(())
    }
}
