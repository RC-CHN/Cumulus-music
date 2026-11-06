//! Ordinary indexed local files cannot inherit a download's transport rights.

use super::{ResolvedSource, SourceResolver};
use crate::{
    Error, Result,
    library::{IndexedFlacReceipt, LocalResource},
    playback::engine::{Cancellation, MediaInput},
};
use serde_json::Value;

impl SourceResolver {
    pub(crate) async fn hold_indexed_local_flac(
        &self,
        resource: &Value,
        source: &ResolvedSource,
        cancel: &Cancellation,
    ) -> Result<Option<(IndexedFlacReceipt, LocalResource)>> {
        let (Some(library), MediaInput::File(path)) = (&self.library, &source.input) else {
            return Ok(None);
        };
        if source.info["kind"] != "local"
            || !source.info["segment"].is_null()
            || !source.info["volume_delta"].is_null()
            || source.info["loudness_invalid"] == true
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("flac"))
        {
            return Ok(None);
        }
        let resource: LocalResource = serde_json::from_value(resource.clone())?;
        let (library, path, cancel) = (library.clone(), path.clone(), cancel.clone());
        tokio::task::spawn_blocking(move || {
            cancel.check()?;
            let receipt = library.hold_ordinary_indexed_flac(&resource, &path, &cancel)?;
            cancel.check()?;
            Ok(receipt.map(|receipt| (receipt, resource)))
        })
        .await
        .map_err(|_| Error::State("indexed FLAC worker stopped"))?
    }

    pub(crate) async fn verify_indexed_local_flac(
        &self,
        resource: &Value,
        source: &ResolvedSource,
        cancel: &Cancellation,
    ) -> Result<()> {
        let (Some(library), MediaInput::File(path)) = (&self.library, &source.input) else {
            return Err(Error::StaleResponse);
        };
        let resource: LocalResource = serde_json::from_value(resource.clone())?;
        let (library, path, cancel) = (library.clone(), path.clone(), cancel.clone());
        tokio::task::spawn_blocking(move || {
            cancel.check()?;
            if !library.ordinary_indexed_flac_current(&resource, &path)? {
                return Err(Error::StaleResponse);
            }
            cancel.check()
        })
        .await
        .map_err(|_| Error::State("indexed FLAC verification worker stopped"))?
    }
}
