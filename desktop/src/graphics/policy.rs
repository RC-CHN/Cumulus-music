use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Mode {
    Auto,
    Gpu,
    GpuShm,
    Software,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::GpuShm => "gpu-shm",
            Self::Software => "software",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "gpu" => Some(Self::Gpu),
            "gpu-shm" => Some(Self::GpuShm),
            "software" => Some(Self::Software),
            _ => None,
        }
    }
}

pub(super) fn candidates(requested: Mode, cached: Option<Mode>, linux: bool) -> Vec<Mode> {
    if requested != Mode::Auto {
        return vec![requested];
    }
    let all = if linux {
        vec![Mode::Auto, Mode::GpuShm, Mode::Software]
    } else {
        vec![Mode::Auto, Mode::Software]
    };
    let start = cached
        .and_then(|mode| all.iter().position(|m| *m == mode))
        .unwrap_or(0);
    all[start..].to_vec()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Outcome {
    Ready,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Serialize)]
pub(super) struct Attempt {
    pub mode: Mode,
    pub outcome: Outcome,
}

/// This loop only invokes a graphics-only child, never the real music client.
pub(super) fn choose(
    modes: &[Mode],
    mut probe: impl FnMut(Mode) -> std::io::Result<Outcome>,
) -> std::io::Result<(Option<Mode>, Vec<Attempt>)> {
    let mut attempts = Vec::new();
    for &mode in modes {
        let outcome = probe(mode)?;
        let ready = outcome == Outcome::Ready;
        let cancelled = outcome == Outcome::Cancelled;
        attempts.push(Attempt { mode, outcome });
        if ready {
            return Ok((Some(mode), attempts));
        }
        if cancelled {
            break;
        }
    }
    Ok((None, attempts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_then_timeout_falls_back_once_and_stops_on_success() {
        let modes = candidates(Mode::Auto, None, true);
        let mut outcomes = [Outcome::Ready, Outcome::TimedOut, Outcome::Failed].to_vec();
        let (selected, attempts) = choose(&modes, |_| Ok(outcomes.pop().unwrap())).unwrap();
        assert_eq!(selected, Some(Mode::Software));
        assert_eq!(attempts.len(), 3);
        assert!(outcomes.is_empty());
        let (selected, attempts) = choose(&modes, |_| Ok(Outcome::Ready)).unwrap();
        assert_eq!(selected, Some(Mode::Auto));
        assert_eq!(attempts.len(), 1);
    }

    #[test]
    fn cache_skips_known_bad_modes_and_manual_overrides_are_pinned() {
        assert_eq!(
            candidates(Mode::Auto, Some(Mode::GpuShm), true),
            [Mode::GpuShm, Mode::Software]
        );
        assert_eq!(
            candidates(Mode::Auto, Some(Mode::Software), false),
            [Mode::Software]
        );
        assert_eq!(
            candidates(Mode::Gpu, Some(Mode::Software), true),
            [Mode::Gpu]
        );
        assert_eq!(
            candidates(Mode::Auto, Some(Mode::GpuShm), false),
            [Mode::Auto, Mode::Software]
        );
    }

    #[test]
    fn exhaustion_and_cancellation_never_relaunch_the_application() {
        let modes = candidates(Mode::Auto, None, true);
        let (selected, attempts) = choose(&modes, |_| Ok(Outcome::Failed)).unwrap();
        assert_eq!(selected, None);
        assert_eq!(attempts.len(), 3);
        let (selected, attempts) = choose(&modes, |_| Ok(Outcome::Cancelled)).unwrap();
        assert_eq!(selected, None);
        assert_eq!(attempts.len(), 1);
        assert!(choose(&modes, |_| Err(std::io::Error::other("spawn failure"))).is_err());
    }
}
