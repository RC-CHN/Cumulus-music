use crate::Error;

/// Bounded diagnostics that can outlive the original failure in player state.
/// Never retain request details, authorization URLs, paths, or mpv messages.
#[derive(Clone, Copy, Debug)]
pub enum PlaybackFailure {
    SourceUnavailable,
    SourceServiceTemporary,
    SessionExpired,
    Network,
    Runtime,
    Output,
    OpenMedia,
    Engine,
    Storage,
    Other,
}

impl PlaybackFailure {
    pub(crate) fn classify(error: &Error) -> Option<Self> {
        Some(match error.root() {
            _ if error.is_service_code(301) => Self::SessionExpired,
            Error::Service { endpoint, code, .. }
                if endpoint == "/api/song/enhance/player/url/v1" =>
            {
                // The endpoint can return service codes both in the envelope
                // and in the matching resource row. Neither throttling nor
                // backend failure is a decision about this song's rights.
                match code.as_i64().or_else(|| code.as_str()?.parse().ok()) {
                    Some(429 | 500..=599) => Self::SourceServiceTemporary,
                    _ => Self::SourceUnavailable,
                }
            }
            Error::Response("no playback resource for requested track") => Self::SourceUnavailable,
            Error::Playback {
                operation: "library" | "library unavailable" | "ABI",
                ..
            } => Self::Runtime,
            // MPV_ERROR_AO_INIT_FAILED; shared with managed output recovery.
            Error::Playback { code: -14, .. } => Self::Output,
            Error::Playback {
                operation: "load ended" | "load timeout" | "unexpected playlist",
                ..
            } => Self::OpenMedia,
            Error::Playback { .. } => Self::Engine,
            Error::Transport(_) | Error::HttpStatus { .. } => Self::Network,
            Error::Storage(_) | Error::Io(_) => Self::Storage,
            _ => return None,
        })
    }

    pub(crate) fn from_error(error: &Error) -> Self {
        Self::classify(error).unwrap_or(Self::Other)
    }
}
