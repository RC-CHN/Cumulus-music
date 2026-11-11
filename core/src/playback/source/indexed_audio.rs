//! Ordinary indexed local files cannot inherit a download's transport rights.

use super::{ResolvedSource, SourceResolver};
use crate::{
    Error, Result,
    library::{IndexedAudioFormat, IndexedAudioReceipt, LocalResource},
    playback::engine::{Cancellation, MediaInput},
};
use serde_json::Value;

impl SourceResolver {
    pub(crate) async fn hold_indexed_local_audio(
        &self,
        resource: &Value,
        source: &ResolvedSource,
        format: IndexedAudioFormat,
        cancel: &Cancellation,
    ) -> Result<Option<(IndexedAudioReceipt, LocalResource)>> {
        let (Some(library), MediaInput::File(path)) = (&self.library, &source.input) else {
            return Ok(None);
        };
        if source.info["kind"] != "local"
            || !source.info["segment"].is_null()
            || !source.info["volume_delta"].is_null()
            || source.info["loudness_invalid"] == true
            || IndexedAudioFormat::from_path(path) != Some(format)
        {
            return Ok(None);
        }
        let resource: LocalResource = serde_json::from_value(resource.clone())?;
        let (library, path, cancel) = (library.clone(), path.clone(), cancel.clone());
        tokio::task::spawn_blocking(move || {
            cancel.check()?;
            let receipt = library.hold_ordinary_indexed_audio(&resource, &path, format, &cancel)?;
            cancel.check()?;
            Ok(receipt.map(|receipt| (receipt, resource)))
        })
        .await
        .map_err(|_| Error::State("indexed audio worker stopped"))?
    }

    pub(crate) async fn verify_indexed_local_audio(
        &self,
        resource: &Value,
        source: &ResolvedSource,
        format: IndexedAudioFormat,
        cancel: &Cancellation,
    ) -> Result<()> {
        let (Some(library), MediaInput::File(path)) = (&self.library, &source.input) else {
            return Err(Error::StaleResponse);
        };
        let resource: LocalResource = serde_json::from_value(resource.clone())?;
        let (library, path, cancel) = (library.clone(), path.clone(), cancel.clone());
        tokio::task::spawn_blocking(move || {
            cancel.check()?;
            if !library.ordinary_indexed_audio_current(&resource, &path, format)? {
                return Err(Error::StaleResponse);
            }
            cancel.check()
        })
        .await
        .map_err(|_| Error::State("indexed audio verification worker stopped"))?
    }
}
