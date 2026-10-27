use super::*;
use crate::download::{
    file_identity::identity,
    service::{DownloadStatus, LyricsStatus},
};
use std::path::{Path, PathBuf};

impl CompletedDownloads {
    pub fn rename(&self, task_id: &str, name: &str) -> Result<FileOperationView> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
            return Err(Error::InvalidInput("rename accepts only a file name"));
        }
        let t = self.token()?;
        self.guard(&t)?;
        let task = self.task(task_id)?;
        let target = task
            .target
            .as_ref()
            .ok_or(Error::State("missing completed target"))?
            .with_file_name(name);
        self.start_for(
            task_id,
            FileAction::Move,
            Some(target),
            t,
            task.target,
            None,
        )
    }
    /// Only records intent. Resume advances the operation; callers retain its ID.
    pub fn start(
        &self,
        task_id: &str,
        action: FileAction,
        target: Option<PathBuf>,
    ) -> Result<FileOperationView> {
        self.start_for(task_id, action, target, self.token()?, None, None)
    }
    pub(in crate::download) fn start_planned(
        &self,
        task_id: &str,
        target: PathBuf,
        id: &str,
        root: &Path,
    ) -> Result<FileOperationView> {
        valid_id(id)?;
        crate::download::directory::paths::prepare_target(&target, root)?;
        self.start_for(
            task_id,
            FileAction::Move,
            Some(target),
            self.token()?,
            None,
            Some(id),
        )
    }
    fn start_for(
        &self,
        task_id: &str,
        action: FileAction,
        target: Option<PathBuf>,
        token: Option<SessionToken>,
        rename_source: Option<PathBuf>,
        operation_id: Option<&str>,
    ) -> Result<FileOperationView> {
        if (action == FileAction::Move) != target.is_some() {
            return Err(Error::InvalidInput("only move accepts a destination"));
        }
        let _locks = self.locks(task_id)?;
        self.guard(&token)?;
        let task = self.task(task_id)?;
        if rename_source.is_some() && task.target != rename_source {
            return Err(Error::State("download moved before rename; refresh first"));
        }
        if task.status != DownloadStatus::Complete || task.file_operation.is_some() {
            return Err(Error::State(
                "file operation needs an idle completed download",
            ));
        }
        if task.cleanup_pending {
            return Err(Error::State(
                "finish download post-processing cleanup before managing files",
            ));
        }
        let mut op = Operation {
            id: operation_id
                .map(str::to_owned)
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
            migration_id: self.migration.as_ref().map(|m| m.0.clone()),
            working: None,
            task_id: task_id.into(),
            action,
            phase: FilePhase::Prepared,
            task: task.clone(),
            target: None,
            files: vec![],
            missing: vec![],
            error: None,
        };
        if action != FileAction::Forget {
            let source = task
                .target
                .as_ref()
                .ok_or(Error::State("completed target is missing"))?;
            let destination = target
                .as_ref()
                .map(|t| fs::normalize_target(t, self.store.directory()))
                .transpose()?;
            if let Some(dest) = &destination {
                if dest == source || extension(dest) != extension(source) {
                    return Err(Error::InvalidInput(
                        "move must preserve format and change path",
                    ));
                }
                fs::absent(dest)?;
            }
            let expected = task
                .output_integrity
                .as_ref()
                .ok_or(Error::State("completed integrity is missing"))?;
            let mut inputs = vec![(
                "audio",
                source.clone(),
                expected.clone(),
                destination.clone(),
            )];
            if task.lyrics_status == LyricsStatus::Complete {
                let lyric = task
                    .lyric
                    .as_ref()
                    .and_then(|a| a.integrity.clone())
                    .ok_or(Error::State("completed lyric integrity is missing"))?;
                inputs.push((
                    "lyric",
                    source.with_extension("lrc"),
                    lyric,
                    destination.as_ref().map(|p| p.with_extension("lrc")),
                ));
            }
            for (role, path, expected, destination) in inputs {
                let actual = match fs::facts(&path) {
                    Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                        if action == FileAction::Move && role == "audio" {
                            return Err(Error::State("download output is missing"));
                        }
                        op.missing.push(role.into());
                        continue;
                    }
                    other => other?,
                };
                if actual.integrity != expected {
                    return Err(Error::State(
                        "download or lyric was modified; refusing removal",
                    ));
                }
                let source_parent =
                    identity(path.parent().ok_or(Error::State("missing source parent"))?)?;
                fs::parent(&path, &source_parent)?;
                let destination_parent = destination
                    .as_ref()
                    .map(|p| {
                        fs::absent(p)?;
                        identity(p.parent().expect("normalized destination"))
                    })
                    .transpose()?;
                op.files.push(ManagedFile {
                    role: role.into(),
                    source: path,
                    source_parent,
                    expected: actual,
                    destination,
                    destination_parent,
                    destination_facts: None,
                    temporary: None,
                    temporary_identity: None,
                    stage: FileStage::Pending,
                    bytes_copied: 0,
                    replacement_retained: false,
                    alias: None,
                });
            }
            op.target = destination;
        }
        self.guarded(&token, || {
            self.store.transaction(|tx| {
                if tx.get(&self.owner, TASKS, task_id)? != Some(serde_json::to_value(&task)?) {
                    return Err(Error::State("download changed before intent commit"));
                }
                if tx.get(&self.owner, JOURNAL, &op.id)?.is_some() {
                    return Err(Error::State("file operation already exists"));
                }
                let mut task = task.clone();
                task.file_operation = Some(op.id.clone());
                tx.put(&self.owner, TASKS, task_id, &serde_json::to_value(task)?)?;
                tx.put(&self.owner, JOURNAL, &op.id, &serde_json::to_value(&op)?)
            })
        })?;
        self.event(FileEvent::Prepared, &op)?;
        self.guard(&token)?;
        Ok(op.view())
    }
}
fn extension(path: &Path) -> Option<String> {
    path.extension().map(|s| s.to_string_lossy().to_lowercase())
}
