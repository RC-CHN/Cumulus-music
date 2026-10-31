//! Recovered M/7 non-heartbeat/together cycles and FM's separate repeat switch.
use super::PlaybackMode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeFamily {
    Ordinary,
    Fm,
    Together,
    Intelligence,
}
impl ModeFamily {
    pub fn choices(self) -> &'static [PlaybackMode] {
        use PlaybackMode::*;
        match self {
            Self::Ordinary => &[Sequential, OrderLoop, SingleLoop, Random],
            Self::Together => &[OrderLoop, SingleLoop, Random],
            Self::Fm => &[Sequential, SingleLoop],
            Self::Intelligence => &[OrderLoop],
        }
    }
    pub fn supports(self, mode: PlaybackMode) -> bool {
        self.choices().contains(&mode)
    }
    pub fn next(self, current: PlaybackMode) -> PlaybackMode {
        let choices = self.choices();
        let index = choices
            .iter()
            .position(|mode| *mode == current)
            .unwrap_or(0);
        choices[(index + 1) % choices.len()]
    }
}
