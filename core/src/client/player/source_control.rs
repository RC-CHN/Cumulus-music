use crate::{
    Error, Result,
    playback::controller::{PlaybackSnapshot, PlaybackStatus},
};

pub(super) fn validate_identity(
    current: &PlaybackSnapshot,
    play_id: &str,
    queue_revision: &str,
) -> Result<()> {
    let revision = queue_revision
        .parse::<u64>()
        .map_err(|_| Error::InvalidInput("invalid source playback queue revision"))?;
    if revision.to_string() != queue_revision {
        return Err(Error::InvalidInput(
            "invalid source playback queue revision",
        ));
    }
    if current.queue_revision != revision || current.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

pub(super) fn validate(
    current: &PlaybackSnapshot,
    play_id: &str,
    queue_revision: &str,
    refresh_failed: bool,
) -> Result<()> {
    validate_identity(current, play_id, queue_revision)?;
    if refresh_failed {
        return if current.state == PlaybackStatus::Error {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        };
    }
    if !matches!(
        current.state,
        PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
    ) {
        return Err(Error::State("source playback is no longer loaded"));
    }
    Ok(())
}
