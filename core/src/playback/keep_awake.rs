//! Official PC playback-to-power request projection. The desktop host owns
//! platform effects and keeps this observation inactive until explicitly wired.
/// Recovered M/7 enum; native buffering was a separate event and did not
/// change this state. Cumulus actor status cannot be substituted mechanically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i8)]
pub enum OfficialPlayingState {
    End = -1,
    Stop = 0,
    Pause = 1,
    Playing = 2,
}

/// The only two payloads emitted by M/473's audio power effect. M/4 sends
/// enable=true for both; clearing both requirements does not mean enable=false.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackPowerRequest {
    Clear,
    KeepSystemAndDisplayAwake,
}

/// M/473 watches the stored setting and the exact Playing predicate, not the
/// current song, playback position, media rights, or the experiment assignment.
#[derive(Debug, Default)]
pub struct PlaybackKeepAwake {
    previous: Option<(bool, bool)>,
}

impl PlaybackKeepAwake {
    /// Settings exposure is separate from the runtime request predicate.
    pub fn windows_setting_visible(experiment_group: &str) -> bool {
        experiment_group == "t1"
    }

    /// Return one request on the first observation and whenever a dependency
    /// changes, even if both old and new values project to Clear.
    pub fn observe(
        &mut self,
        disable_system_sleep: bool,
        state: OfficialPlayingState,
    ) -> Option<PlaybackPowerRequest> {
        let playing = state == OfficialPlayingState::Playing;
        let next = (disable_system_sleep, playing);
        if self.previous.replace(next) == Some(next) {
            return None;
        }
        Some(if disable_system_sleep && playing {
            PlaybackPowerRequest::KeepSystemAndDisplayAwake
        } else {
            PlaybackPowerRequest::Clear
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use OfficialPlayingState::{End, Pause, Playing, Stop};
    use PlaybackPowerRequest::{Clear, KeepSystemAndDisplayAwake as Keep};

    #[test]
    fn runtime_observes_effect_dependencies_and_only_exact_playing() {
        let mut policy = PlaybackKeepAwake::default();
        let observations = [
            (false, Stop, Some(Clear)), // The initial render still sends a clear.
            (false, Pause, None),       // Non-Playing to non-Playing is not a dependency change.
            (false, Playing, Some(Clear)),
            (true, Playing, Some(Keep)),
            (true, Playing, None), // Track/position/buffering alone are not dependencies.
            (true, Pause, Some(Clear)),
            (true, Stop, None),
            (true, Playing, Some(Keep)),
            (true, End, Some(Clear)),
            (false, End, Some(Clear)),
        ];
        for (setting, status, expected) in observations {
            assert_eq!(policy.observe(setting, status), expected);
        }
    }

    #[test]
    fn settings_exposure_does_not_override_a_stored_enabled_preference() {
        assert!(PlaybackKeepAwake::windows_setting_visible("t1"));
        for group in ["", "T1", "control", "t2"] {
            assert!(!PlaybackKeepAwake::windows_setting_visible(group));
            assert_eq!(
                PlaybackKeepAwake::default().observe(true, Playing),
                Some(Keep)
            );
        }
    }
}
