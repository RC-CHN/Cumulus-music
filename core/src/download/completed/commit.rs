use super::*;
use crate::{
    download::{
        file_identity::identity,
        offline::OfflineRecord,
        service::{DownloadDestination, LyricsStatus, TASK_CONTROL, files::Artifact},
    },
    library::{identity::fingerprint, relocate_download},
};
use execute::current_destination;

impl CompletedDownloads {
    pub(super) fn commit(&self, op: &mut Operation, token: &Option<SessionToken>) -> Result<()> {
        if op.working.is_some() {
            return self.commit_work(op, token);
        }
        let mut committed = op.clone();
        committed.phase = FilePhase::Cleanup;
        committed.error = None;
        for f in &op.files {
            if op.action == FileAction::Move {
                current_destination(f)?;
            }
        }
        let new_fp = op.target.as_deref().map(fingerprint).transpose()?;
        self.guarded(token, || {
            self.store.transaction(|tx| {
                let mut expected = op.task.clone();
                expected.file_operation = Some(op.id.clone());
                if tx.get(&self.owner, TASKS, &op.task_id)?
                    != Some(serde_json::to_value(&expected)?)
                {
                    return Err(Error::State(
                        "download record changed during file operation",
                    ));
                }
                if op.action == FileAction::Move {
                    let target = op.target.as_ref().expect("move target");
                    let audio = op
                        .files
                        .iter()
                        .find(|f| f.role == "audio")
                        .ok_or(Error::State("missing moved audio"))?;
                    let facts = audio.destination_facts.as_ref().expect("ready audio");
                    expected.target = Some(target.clone());
                    expected.destination = DownloadDestination::Path {
                        path: target.clone(),
                    };
                    expected.root = target.parent().expect("parent").to_path_buf();
                    expected.root_identity = identity(&expected.root)?;
                    expected.work = expected.root.join(format!(".cumulus-{}", expected.id));
                    expected.target_parent = Some(expected.root_identity.clone());
                    // Completed tasks no longer have a working artifact; publication
                    // verification uses the recorded output identity at its new path.
                    expected.prepared = Some(Artifact {
                        path: expected.work.join("prepared"),
                        identity: facts.identity.clone(),
                        integrity: Some(facts.integrity.clone()),
                    });
                    if expected.lyrics_status == LyricsStatus::Complete {
                        if let Some(lyric) = op.files.iter().find(|f| f.role == "lyric") {
                            let facts = lyric.destination_facts.as_ref().expect("ready lyric");
                            expected.lyric = Some(Artifact {
                                path: expected.work.join("lyric"),
                                identity: facts.identity.clone(),
                                integrity: Some(facts.integrity.clone()),
                            });
                        } else {
                            expected.lyrics_status = LyricsStatus::Missing;
                            expected.lyric = None;
                        }
                    }
                    tx.put(
                        &self.owner,
                        TASKS,
                        &op.task_id,
                        &serde_json::to_value(&expected)?,
                    )?;
                    relocate_download(
                        tx,
                        &audio.source,
                        Some((target, new_fp.clone().expect("move fingerprint"))),
                        Some(&audio.expected.fingerprint),
                    )?;
                } else {
                    tx.delete(&self.owner, TASKS, &op.task_id)?;
                    tx.delete(&self.owner, "media_transfers_v1", &op.task_id)?;
                    tx.delete(&self.owner, TASK_CONTROL, &op.task_id)?;
                    if op.action == FileAction::Delete {
                        relocate_download(
                            tx,
                            op.task
                                .target
                                .as_deref()
                                .ok_or(Error::State("missing target"))?,
                            None,
                            None,
                        )?;
                    }
                }
                for row in tx.rows("offline_music_v1")? {
                    if row.owner != self.owner {
                        continue;
                    }
                    let mut record: OfflineRecord = serde_json::from_value(row.value)?;
                    if record.task_id != op.task_id {
                        continue;
                    }
                    if op.action == FileAction::Forget {
                        crate::download::offline::detach_download_queue(tx, &self.owner, &record)?;
                    }
                    if let Some(target) = &op.target {
                        record.path = target.clone();
                        record.id = fs::path_key(target)?;
                        record.revision = uuid::Uuid::new_v4().to_string();
                        if tx
                            .get(&self.owner, "offline_music_v1", &record.id)?
                            .is_some()
                        {
                            return Err(Error::State("destination already has an offline record"));
                        }
                        tx.put(
                            &self.owner,
                            "offline_music_v1",
                            &record.id,
                            &serde_json::to_value(&record)?,
                        )?;
                    }
                    tx.delete(&self.owner, "offline_music_v1", &row.key)?;
                }
                tx.put(
                    &self.owner,
                    JOURNAL,
                    &op.id,
                    &serde_json::to_value(&committed)?,
                )
            })
        })?;
        *op = committed;
        Ok(())
    }
    pub(super) fn finish(
        &self,
        op: &mut Operation,
        token: &Option<SessionToken>,
        phase: FilePhase,
    ) -> Result<()> {
        let mut finished = op.clone();
        finished.phase = phase;
        finished.error = None;
        self.guarded(token, || {
            self.store.transaction(|tx| {
                if let Some(value) = tx.get(&self.owner, TASKS, &op.task_id)? {
                    let mut task: DownloadTask = serde_json::from_value(value)?;
                    if task.file_operation.as_deref() != Some(&op.id) {
                        return Err(Error::State("download no longer owns this operation"));
                    }
                    task.file_operation = None;
                    tx.put(
                        &self.owner,
                        TASKS,
                        &op.task_id,
                        &serde_json::to_value(task)?,
                    )?;
                } else if op.action == FileAction::Move || phase == FilePhase::Cancelled {
                    return Err(Error::State("download record disappeared before finish"));
                }
                tx.put(
                    &self.owner,
                    JOURNAL,
                    &op.id,
                    &serde_json::to_value(&finished)?,
                )
            })
        })?;
        *op = finished;
        Ok(())
    }
}
