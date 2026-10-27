use super::*;
impl CompletedDownloads {
    pub(in crate::download) fn start_work(
        &self,
        task_id: &str,
        source: &Path,
        target: &Path,
        id: &str,
    ) -> Result<FileOperationView> {
        valid_id(id)?;
        let token = self.token()?;
        let _locks = self.locks(task_id)?;
        self.guard(&token)?;
        let task = self.task(task_id)?;
        if task.file_operation.is_some()
            || task.status == DownloadStatus::Cancelled
            || (task.status == DownloadStatus::Complete && !task.cleanup_pending)
        {
            return Err(Error::State("task does not need working-file relocation"));
        }
        let inputs = self.working_inputs(&task, source, target)?;
        let mut after = task.clone();
        after.root = mapped(&task.root, source, target)?;
        after.work = mapped(&task.work, source, target)?;
        crate::download::directory::paths::ensure_directory(&after.root, target)?;
        if after.root.canonicalize()? != after.root {
            return Err(Error::State("new work root contains a link"));
        }
        after.root_identity = identity(&after.root)?;
        let mut builder = stdfs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&after.work) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            r => r?,
        }
        if !stdfs::symlink_metadata(&after.work)?.is_dir()
            || stdfs::read_dir(&after.work)?.next().is_some()
        {
            return Err(Error::State("destination work directory is not empty"));
        }
        after.work_identity = identity(&after.work)?;
        after.target = task
            .target
            .as_deref()
            .map(|p| mapped(p, source, target))
            .transpose()?;
        if let Some(path) = &after.target {
            crate::download::directory::paths::prepare_target(path, target)?;
            after.target_parent = Some(identity(path.parent().expect("target parent"))?);
        }
        after.destination = match &task.destination {
            DownloadDestination::Path { path } => DownloadDestination::Path {
                path: mapped(path, source, target)?,
            },
            DownloadDestination::Named {
                root,
                path_type,
                file_type,
            } => DownloadDestination::Named {
                root: mapped(root, source, target)?,
                path_type: *path_type,
                file_type: *file_type,
            },
        };
        if after.target.is_none()
            && let DownloadDestination::Path { path } = &after.destination
        {
            crate::download::directory::paths::prepare_target(path, target)?;
            fs::absent(path)?;
        }
        for a in [&mut after.prepared, &mut after.cover, &mut after.lyric]
            .into_iter()
            .flatten()
        {
            a.path = mapped(&a.path, source, target)?;
        }
        let raw_before = self.raw(task_id)?;
        let mut raw_after = raw_before.clone();
        if let Some(raw) = &mut raw_after {
            raw.target = mapped(&raw.target, source, target)?;
            raw.part = mapped(&raw.part, source, target)?;
            raw.parent_token = after.work_identity.clone();
        }
        let mut op = Operation {
            id: id.into(),
            task_id: task_id.into(),
            action: FileAction::Move,
            phase: FilePhase::Prepared,
            task: task.clone(),
            target: after.target.clone(),
            files: vec![],
            missing: vec![],
            error: None,
            migration_id: self.migration.as_ref().map(|m| m.0.clone()),
            working: Some(Working {
                new_work_identity: after.work_identity.clone(),
                after,
                raw_before,
                raw_after,
                old_work_retained: false,
            }),
        };
        for input in inputs {
            let facts = fs::facts(&input.source)?;
            crate::download::directory::paths::prepare_target(&input.target, target)?;
            fs::absent(&input.target)?;
            let alias = op
                .files
                .iter()
                .position(|f| f.expected.identity == facts.identity);
            op.files.push(ManagedFile {
                role: input.role,
                source_parent: identity(input.source.parent().expect("source parent"))?,
                source: input.source,
                destination_parent: Some(identity(input.target.parent().expect("target parent"))?),
                destination: Some(input.target),
                expected: facts,
                destination_facts: None,
                temporary: None,
                temporary_identity: None,
                stage: FileStage::Pending,
                bytes_copied: 0,
                replacement_retained: false,
                alias,
            });
        }
        fs::sync(&op.working.as_ref().expect("working").after.root)?;
        self.guarded(&token, || {
            self.store.transaction(|tx| {
                if tx.get(&self.owner, TASKS, task_id)? != Some(serde_json::to_value(&task)?)
                    || tx.get(&self.owner, JOURNAL, id)?.is_some()
                {
                    return Err(Error::State("task changed before relocation intent"));
                }
                let mut task = task.clone();
                task.file_operation = Some(id.into());
                tx.put(&self.owner, TASKS, task_id, &serde_json::to_value(task)?)?;
                tx.put(&self.owner, JOURNAL, id, &serde_json::to_value(&op)?)
            })
        })?;
        self.event(FileEvent::Prepared, &op)?;
        self.guard(&token)?;
        Ok(op.view())
    }
}
