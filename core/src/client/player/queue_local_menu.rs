//! Indexed local rows carry their own original file in M556's playing-list menu.
use super::{MusicClient, PlayerResourceKind, identity};
use crate::{
    Error, Result,
    account::SessionToken,
    library::{LocalLibrary, LocalRevision},
    playback::QueueEntry,
};
use std::path::{Path, PathBuf};

pub(super) fn eligible(entry: &QueueEntry) -> bool {
    let resource = &entry.resource;
    identity::kind(resource) == PlayerResourceKind::Local
        && entry.source["nativeOpen"] != true
        && [
            "native_open_file",
            "cloud_song_id",
            "voice_id",
            "voice",
            "download_voice_id",
        ]
        .iter()
        .all(|key| resource.get(key).is_none())
        && resource["local_revision"].is_object()
        && resource["local_id"]
            .as_str()
            .is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl MusicClient {
    async fn indexed_queue_entry(
        &self,
        token: &SessionToken,
        revision: u64,
        entry_id: &str,
    ) -> Result<QueueEntry> {
        let (state, queue) = self
            .player
            .controller()
            .await?
            .queue_with_state(token)
            .await?;
        if state.queue_revision != revision {
            return Err(Error::QueueChanged);
        }
        if state.fm
            || queue.intelligence.is_some()
            || self.queue_clear_context(token)?.room_id.is_some()
        {
            return Err(Error::State(
                "local file reveal requires the ordinary queue",
            ));
        }
        let entry = queue
            .items
            .into_iter()
            .find(|entry| entry.entry_id == entry_id && entry.visible_in_queue() && eligible(entry))
            .ok_or(Error::InvalidInput(
                "queue file reveal requires an indexed local track",
            ))?;
        self.session.check_generation(token)?;
        Ok(entry)
    }

    async fn indexed_queue_path(&self, id: String, revision: LocalRevision) -> Result<PathBuf> {
        let context = self.local.context().await?;
        tokio::task::spawn_blocking(move || {
            const FILE_CHANGED: Error = Error::State("queued local file is unavailable");
            let item = context.library.item(&id).map_err(|error| match error {
                Error::State("local track is not available") => FILE_CHANGED,
                other => other,
            })?;
            if item.revision != revision {
                return Err(Error::QueueChanged);
            }
            let same_path = |path: &Path| path.canonicalize().is_ok_and(|actual| actual == path);
            if !same_path(&item.path)
                || matches!(&revision, LocalRevision::Cue { sheet, .. } if !same_path(sheet))
            {
                return Err(FILE_CHANGED);
            }
            LocalLibrary::check_current(&item).map_err(|_| FILE_CHANGED)?;
            Ok(item.path)
        })
        .await
        .map_err(|_| Error::State("queue file validation stopped"))?
    }

    pub async fn player_queue_local_file_reveal(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
    ) -> Result<QueueLocalFileReveal> {
        let token = self.player_token(epoch)?;
        let version = revision
            .parse()
            .map_err(|_| Error::InvalidInput("invalid queue file revision"))?;
        let entry = self.indexed_queue_entry(&token, version, entry_id).await?;
        let id = entry.resource["local_id"]
            .as_str()
            .expect("indexed local identity")
            .to_owned();
        let revision =
            serde_json::from_value::<LocalRevision>(entry.resource["local_revision"].clone())
                .map_err(|_| Error::InvalidInput("invalid indexed local file revision"))?;
        let path = self
            .indexed_queue_path(id.clone(), revision.clone())
            .await?;
        if self.indexed_queue_entry(&token, version, entry_id).await? != entry {
            return Err(Error::QueueChanged);
        }
        Ok(QueueLocalFileReveal {
            token,
            version,
            entry,
            id,
            revision,
            path,
        })
    }
}

/// Native file-manager receipt; no file path is returned to the renderer.
pub struct QueueLocalFileReveal {
    token: SessionToken,
    version: u64,
    entry: QueueEntry,
    id: String,
    revision: LocalRevision,
    path: PathBuf,
}

impl QueueLocalFileReveal {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn check_current(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        if client
            .indexed_queue_path(self.id.clone(), self.revision.clone())
            .await?
            != self.path
            || client
                .indexed_queue_entry(&self.token, self.version, &self.entry.entry_id)
                .await?
                != self.entry
        {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
}
