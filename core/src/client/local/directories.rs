//! Device-local directory selection. No account or network authorization.
use super::{DirectoryChoice, DirectorySelection, MusicClient};
use crate::{Error, Result};
use std::path::PathBuf;

impl MusicClient {
    pub async fn local_directory_selection(&self) -> Result<DirectorySelection> {
        let local = self.local.context().await?;
        tokio::task::spawn_blocking(move || local.monitor.directory_selection())
            .await
            .map_err(|_| Error::State("local directory worker stopped"))?
    }

    /// The host supplies an explicitly selected native folder, not a path
    /// from a renderer request or one derived from a title.
    pub async fn stage_local_directory(
        &self,
        path: PathBuf,
        revision: String,
    ) -> Result<DirectorySelection> {
        validate_revision(&revision)?;
        let local = self.local.context().await?;
        tokio::task::spawn_blocking(move || local.monitor.stage_directory(&path, &revision))
            .await
            .map_err(|_| Error::State("local directory worker stopped"))?
    }

    pub async fn apply_local_directories(
        &self,
        revision: String,
        choices: Vec<DirectoryChoice>,
    ) -> Result<DirectorySelection> {
        validate_revision(&revision)?;
        let local = self.local.context().await?;
        tokio::task::spawn_blocking(move || local.monitor.apply_directories(&revision, choices))
            .await
            .map_err(|_| Error::State("local directory worker stopped"))?
    }
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.len() != 64 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput(
            "invalid local directory selection revision",
        ));
    }
    Ok(())
}
