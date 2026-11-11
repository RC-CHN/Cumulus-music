//! The history row binds the episode, while the download service owns its media.
use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::HistorySelectionScope,
    download::service::{ScopedTaskAdmission, ScopedTaskLease},
    history::{HistoryKind, HistoryService},
    podcast::Episode,
    storage::{PlaybackDownloadKind, StateTransaction},
};

pub(super) struct HistoryVoiceRow {
    pub(super) id: String,
    media: Option<String>,
}

impl MusicClient {
    pub(super) fn history_voice_download_rows(
        &self,
        token: &SessionToken,
        target: &HistorySelectionScope,
        maximum: usize,
    ) -> Result<Vec<HistoryVoiceRow>> {
        let snapshot =
            HistoryService::new(self.session.clone())?.snapshot_for(token, HistoryKind::Voice)?;
        target
            .ordered_rows(&snapshot, maximum)?
            .into_iter()
            .map(|row| {
                let item = Episode::from_official(&row.resource)?;
                if item.id != row.id {
                    return Err(Error::QueueChanged);
                }
                Ok(HistoryVoiceRow {
                    id: item.id,
                    media: item.song_id,
                })
            })
            .collect()
    }

    pub(super) fn verify_history_voice_download(
        &self,
        token: &SessionToken,
        target: &HistorySelectionScope,
    ) -> Result<()> {
        let snapshot =
            HistoryService::new(self.session.clone())?.snapshot_for(token, HistoryKind::Voice)?;
        if snapshot.revision != target.revision {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }

    pub(super) async fn filter_history_voice_downloads(
        &self,
        epoch: &str,
        rows: Vec<HistoryVoiceRow>,
    ) -> Result<Vec<String>> {
        let context = self.downloads.inner.context(epoch)?;
        let local = self.local.context().await?;
        tokio::task::spawn_blocking(move || {
            let wanted = rows.iter().filter_map(|row| row.media.as_deref()).collect();
            let (_local, matched) = local.library.hold_online_song_matches(
                &context.session,
                &context.token,
                &wanted,
            )?;
            context.guard()?;
            let ids = context.store.read_snapshot(|tx| {
                let mut ids = Vec::new();
                for row in rows {
                    if row.media.as_ref().is_none_or(|id| !matched.contains(id))
                        && !task_exists(tx, &context.owner, &row.id)?
                    {
                        ids.push(row.id);
                    }
                }
                Ok(ids)
            })?;
            context.guard()?;
            Ok(ids)
        })
        .await
        .map_err(|_| Error::State("history episode admission worker stopped"))?
    }
}

pub(super) struct HistoryVoiceAdmission<'a> {
    pub(super) target: &'a HistorySelectionScope,
    pub(super) voice: &'a str,
}

impl ScopedTaskAdmission for HistoryVoiceAdmission<'_> {
    fn acquire(&self, _media_id: &str) -> Result<Box<dyn ScopedTaskLease + '_>> {
        // The voice service has already resolved this exact programme and
        // selected its current media ID. This lease fences the original row.
        Ok(Box::new(self))
    }
}

impl ScopedTaskLease for &HistoryVoiceAdmission<'_> {
    fn verify(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        self.target.verify_in(tx, owner, HistoryKind::Voice)?;
        if task_exists(tx, owner, self.voice)? {
            return Err(Error::State("program already has a download"));
        }
        Ok(())
    }
}

fn task_exists(tx: &StateTransaction<'_>, owner: &str, episode: &str) -> Result<bool> {
    Ok(tx
        .playback_download_rows(owner, episode, PlaybackDownloadKind::Voice)?
        .iter()
        .any(|row| row.status != "cancelled"))
}
