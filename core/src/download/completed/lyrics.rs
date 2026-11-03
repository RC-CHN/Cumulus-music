//! Read-only proof that an inline playback ledger came from a completed Forget.
use super::*;
use crate::download::{
    offline::{OfflineLibrary, OfflineRecord, PublishedDownload},
    service::files,
};

const PAGE: usize = 128;
const MAX_PAGES: usize = 16;

impl CompletedDownloads {
    pub(crate) fn read_forgotten_lyrics(
        &self,
        token: &SessionToken,
        record: &OfflineRecord,
        cloud: bool,
    ) -> Result<Option<String>> {
        let token_ref = Some(token.clone());
        self.guard(&token_ref)?;
        if !record.path.is_absolute()
            || record.id != fs::path_key(&record.path)?
            || uuid::Uuid::parse_str(&record.revision).is_err()
        {
            return Ok(None);
        }
        // Shared with completed moves, directory migration and cloud/local
        // removal. No lyric API request is made while holding these locks.
        let _locks = self.locks(&record.task_id)?;
        let Some(task) = self.forgotten_task(&token_ref, record, cloud)? else {
            return Ok(None);
        };
        if task.playback_target()? != record.path || files::target(&task)? != record.path {
            return Ok(None);
        }
        let (Some(song), Some(resource)) = (task.song.as_ref(), task.resource.as_ref()) else {
            return Ok(None);
        };
        let publication = PublishedDownload {
            task_id: task.id.clone(),
            song_id: task.song_id.clone(),
            requested_quality: task.options.quality,
            target: record.path.clone(),
            song: song.clone(),
            resource: resource.clone(),
            encrypted: task.encrypted,
            integrity: record.integrity.clone(),
        };
        let mut expected = publication.record(record.path.clone(), record.id.clone())?;
        expected.revision.clone_from(&record.revision);
        if &expected != record {
            return Ok(None);
        }
        let Some(prepared) = &task.prepared else {
            return Ok(None);
        };
        let facts = fs::recorded_facts(&record.path, record.integrity.clone())?;
        if prepared.integrity.as_ref() != Some(&record.integrity)
            || facts.identity != prepared.identity
        {
            return Ok(None);
        }
        let offline = OfflineLibrary::new(self.session.clone().ok_or(Error::StaleResponse)?)?;
        if !offline.detached_access(token, record)?.can_play {
            return Ok(None);
        }
        crate::download::offline::verify_detached_audio(record)?;
        fs::unchanged(&record.path, &facts)?;
        files::target(&task)?;
        let text = crate::lyrics::files::read_published_sidecar(&record.path)?;
        files::target(&task)?;
        fs::unchanged(&record.path, &facts)?;
        if !offline.detached_access(token, record)?.can_play {
            return Ok(None);
        }
        self.guard(&token_ref)?;
        Ok(text)
    }

    fn forgotten_task(
        &self,
        token: &Option<SessionToken>,
        record: &OfflineRecord,
        cloud: bool,
    ) -> Result<Option<DownloadTask>> {
        self.guarded(token, || {
            self.store.transaction(|tx| {
                if tx.get(&self.owner, TASKS, &record.task_id)?.is_some()
                    || tx
                        .get(&self.owner, "offline_music_v1", &record.id)?
                        .is_some()
                {
                    // Intelligence also uses inline records while a task is
                    // still published. That isn't a completed Forget.
                    return Ok(None);
                }
                let mut after = None;
                for _ in 0..MAX_PAGES {
                    let page = tx.values_page(&self.owner, JOURNAL, after.as_deref(), PAGE)?;
                    for (key, value) in &page {
                        if value["task_id"].as_str() != Some(&record.task_id)
                            || value["action"] != "forget"
                            || value["phase"] != "complete"
                        {
                            continue;
                        }
                        let operation: Operation = serde_json::from_value(value.clone())?;
                        let task = operation.task;
                        if operation.id == *key
                            && operation.action == FileAction::Forget
                            && operation.phase == FilePhase::Complete
                            && operation.target.is_none()
                            && operation.files.is_empty()
                            && task.id == record.task_id
                            && task.song_id == record.song_id
                            && task.target.as_deref() == Some(record.path.as_path())
                            && task.output_integrity.as_ref() == Some(&record.integrity)
                            && task.encrypted == record.encrypted
                            && task.origin.is_cloud() == cloud
                            && !task.origin.is_mv()
                            && !task.origin.is_voice()
                        {
                            return Ok(Some(task));
                        }
                    }
                    if page.len() < PAGE {
                        break;
                    }
                    after = page.last_key_value().map(|(key, _)| key.clone());
                }
                Ok(None)
            })
        })
    }
}
