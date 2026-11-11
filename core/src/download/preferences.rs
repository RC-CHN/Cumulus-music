//! Account defaults for future downloads, never an existing task or a license.
use super::service::DownloadOptions;
use crate::{Error, Result, membership::Quality, storage::StateTransaction};

pub(crate) const KIND: &str = "download_preferences_v1";

pub(crate) fn clamp_member_default(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    ceiling: Quality,
) -> Result<()> {
    let mut stored = match tx.get(owner, KIND, "options") {
        Ok(Some(stored)) => stored,
        Ok(None) | Err(Error::Json(_)) => return Ok(()),
        Err(error) => return Err(error),
    };
    let Ok(options) = serde_json::from_value::<DownloadOptions>(stored.clone()) else {
        return Ok(()); // Damaged defaults still require their own explicit repair.
    };
    if options.quality.bitrate() > ceiling.bitrate() {
        // Change exactly the authorized setting; retain unrelated or newer
        // option fields in the original persisted record.
        stored["quality"] = serde_json::to_value(ceiling)?;
        tx.put(owner, KIND, "options", &stored)?;
    }
    Ok(())
}
