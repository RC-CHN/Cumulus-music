//! The exact downloaded wrapper owns its path (M556:839–843).
use super::{MusicClient, PlayerResourceKind, QueueDownloadMenuKind, identity, queue_like};
use crate::{
    Error, Result, account::SessionToken, download::completed::DownloadFileReveal,
    playback::QueueEntry, protocol::api::identifier,
};
use std::path::Path;

struct Target<'a> {
    task: &'a str,
    path: &'a Path,
    media: String,
    voice: Option<String>,
}

fn target(entry: &QueueEntry) -> Option<Target<'_>> {
    let resource = &entry.resource;
    if identity::kind(resource) != PlayerResourceKind::Downloaded
        || ["local_id", "cloud_song_id", "voice_id"]
            .iter()
            .any(|key| !resource[key].is_null())
    {
        return None;
    }
    let task = resource["download_task_id"].as_str()?;
    let path = Path::new(resource["path"].as_str()?);
    if task.len() != 32 || !task.bytes().all(|b| b.is_ascii_hexdigit()) || !path.is_absolute() {
        return None;
    }
    let (media, voice) = if resource["download_voice_id"].is_null() {
        (queue_like::song_id(resource)?, None)
    } else {
        // Native download_resources projects both identities from the typed
        // task. Rich podcast metadata is not needed to reveal that file.
        let episode = identifier(&resource["download_voice_id"]).ok()?;
        let media = identifier(&resource["id"]).ok()?;
        if identifier(&resource["song"]["id"]).ok()? != media
            || identifier(&resource["song"]["voice_id"]).ok()? != episode
        {
            return None;
        }
        (media, Some(episode))
    };
    Some(Target {
        task,
        path,
        media,
        voice,
    })
}

pub(super) fn kind(entry: &QueueEntry) -> Option<QueueDownloadMenuKind> {
    Some(if target(entry)?.voice.is_some() {
        QueueDownloadMenuKind::Voice
    } else {
        QueueDownloadMenuKind::Song
    })
}

impl MusicClient {
    pub async fn player_queue_download_file_reveal(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
    ) -> Result<QueueDownloadFileReveal> {
        let token = self.player_token(epoch)?;
        if token.uid.is_none() {
            return Err(Error::StaleResponse);
        }
        let version = revision
            .parse()
            .map_err(|_| Error::InvalidInput("invalid queue file revision"))?;
        let entry = self
            .ordinary_file_queue_entry(&token, version, entry_id)
            .await?;
        let target = target(&entry).ok_or(Error::InvalidInput(
            "queue file reveal requires an exact audio download",
        ))?;
        let file = self
            .download_file_reveal_at(epoch, target.task.into(), target.path.to_owned())
            .await
            .map_err(file_error)?;
        let matched = match &target.voice {
            Some(episode) => file.matches_voice_association(episode, &target.media),
            None => file.matches_song_association(&target.media),
        };
        if file.path() != target.path
            || !matched
            || self
                .ordinary_file_queue_entry(&token, version, entry_id)
                .await?
                != entry
        {
            return Err(Error::QueueChanged);
        }
        Ok(QueueDownloadFileReveal {
            token,
            version,
            entry,
            file,
        })
    }
}

fn file_error(error: Error) -> Error {
    match error {
        Error::Media(_) | Error::Io(_) | Error::State(_) | Error::MutationBusy => {
            Error::State("queued download file is unavailable")
        }
        other => other,
    }
}

/// Receipt for one exact queued publication, not for the same song elsewhere.
pub struct QueueDownloadFileReveal {
    token: SessionToken,
    version: u64,
    entry: QueueEntry,
    file: DownloadFileReveal,
}

impl QueueDownloadFileReveal {
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub async fn check_current(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        self.file.check_current().map_err(file_error)?;
        if client
            .ordinary_file_queue_entry(&self.token, self.version, &self.entry.entry_id)
            .await?
            != self.entry
        {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
}
