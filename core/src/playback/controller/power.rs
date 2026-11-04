//! Translate accepted native audio transitions, never a renderer clock, into M/473 intent.
use super::{PlaybackStatus, state::Actor};
use crate::playback::keep_awake::OfficialPlayingState::{self, End, Pause, Playing, Stop};

impl Actor {
    pub(super) fn power_set(&mut self, intent: OfficialPlayingState) {
        if let Some(power) = &mut self.power {
            let intent = if intent == Playing && self.session.check_generation(&self.token).is_err()
            {
                Stop
            } else {
                intent
            };
            power.set(intent);
        }
    }

    pub(super) fn power_pending(&mut self) {
        if let Some(pending) = &self.pending {
            if pending.paused {
                self.power_set(Pause);
            } else if pending.authorized_position.is_some() {
                self.power_set(Playing);
            }
        }
    }

    pub(super) fn power_after_event(&mut self) {
        if self.power.is_none() {
            return;
        }
        if self.session.check_generation(&self.token).is_err() {
            self.power_set(Stop);
            return;
        }
        match self.state {
            PlaybackStatus::Idle | PlaybackStatus::Error => self.power_set(Stop),
            PlaybackStatus::Paused => self.power_set(Pause),
            PlaybackStatus::Ended => self.power_set(End),
            PlaybackStatus::Playing | PlaybackStatus::Buffering => self.power_set(Playing),
            PlaybackStatus::Preparing | PlaybackStatus::Loading => {
                // A newly requested source has no authority until the native
                // resolver returns. A sequential handoff may retain Playing
                // from the previous authorized song, as the official model did.
                if let Some(power) = &mut self.power {
                    power.observe();
                }
            }
        }
    }
}
