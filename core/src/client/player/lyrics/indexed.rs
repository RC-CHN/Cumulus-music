//! M/213 local playback keeps the original file and optional onlineTrack;
//! M/308 + M/3081 select the matching lyric source independently of audio.
use super::DownloadLyricSource;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::MusicClient,
    download::service::DownloadService,
    library::LocalRevision,
    lyrics::LyricTarget,
};

#[derive(Clone, PartialEq)]
pub(super) struct IndexedLyricSelection {
    id: String,
    revision: LocalRevision,
    play_id: String,
    pub(super) target: LyricTarget,
}

impl MusicClient {
    pub(super) async fn indexed_lyric_selection(
        &self,
        token: &SessionToken,
        play_id: &str,
        id: String,
        revision: LocalRevision,
    ) -> Result<IndexedLyricSelection> {
        let context = self.local.context().await?;
        let session = self.session.clone();
        let expected = token.clone();
        let play_id = play_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let (path, song) = context
                .library
                .indexed_lyric_source(&session, &expected, &id, &revision)?;
            Ok(IndexedLyricSelection {
                id,
                revision,
                play_id,
                target: song.map_or(LyricTarget::Local(path), LyricTarget::Song),
            })
        })
        .await
        .map_err(|_| Error::State("local lyric worker stopped"))?
    }

    pub(super) async fn require_indexed_lyric_selection(
        &self,
        token: &SessionToken,
        indexed: IndexedLyricSelection,
    ) -> Result<()> {
        let current = self
            .indexed_lyric_selection(
                token,
                &indexed.play_id,
                indexed.id.clone(),
                indexed.revision.clone(),
            )
            .await?;
        if current != indexed {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

pub(super) fn downloaded_lyric_source(
    session: &AccountSession,
    token: &SessionToken,
    song: &str,
) -> Result<Option<DownloadLyricSource>> {
    // M/3081's fallback is the independently downloaded track's sidecar.
    // An arbitrary indexed file with the same mapping is not that download.
    let task = match DownloadService::published_catalog_song(session, token, song) {
        Ok(task) => task,
        Err(error @ Error::StaleResponse) => return Err(error),
        Err(_) => return Ok(None),
    };
    Ok(task.and_then(|task| {
        Some(DownloadLyricSource {
            audio: task.playback_target().ok()?.to_path_buf(),
            task_id: task.id,
            song_id: task.song_id,
            cloud: false,
            forgotten: None,
        })
    }))
}
