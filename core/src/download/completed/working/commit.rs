use super::*;
impl CompletedDownloads {
    pub(in crate::download::completed) fn commit_work(
        &self,
        op: &mut Operation,
        token: &Option<SessionToken>,
    ) -> Result<()> {
        let work = op.working.as_ref().expect("working");
        task_files::work(&work.after)?;
        for f in &op.files {
            execute::current_destination(f)?;
        }
        let mut after = work.after.clone();
        after.file_operation = Some(op.id.clone());
        for a in [&mut after.prepared, &mut after.cover, &mut after.lyric]
            .into_iter()
            .flatten()
        {
            if let Some(f) = op.files.iter().find(|f| f.expected.identity == a.identity) {
                a.identity = f
                    .destination_facts
                    .as_ref()
                    .expect("ready")
                    .identity
                    .clone();
            }
        }
        let mut raw = work.raw_after.clone();
        if let Some(raw) = &mut raw
            && let Some(f) = op
                .files
                .iter()
                .find(|f| f.expected.identity == raw.part_token)
        {
            raw.part_token = f
                .destination_facts
                .as_ref()
                .expect("ready")
                .identity
                .clone();
        }
        let audio = op.files.iter().find(|f| f.role == "audio");
        let new_fp = audio
            .map(|f| fingerprint(f.destination.as_deref().expect("destination")))
            .transpose()?;
        let mut committed = op.clone();
        committed.phase = FilePhase::Cleanup;
        committed.error = None;
        self.guarded(token, || {
            self.store.transaction(|tx| {
                let mut expected = op.task.clone();
                expected.file_operation = Some(op.id.clone());
                if tx.get(&self.owner, TASKS, &op.task_id)? != Some(serde_json::to_value(expected)?)
                    || tx.get(&self.owner, RAW, &op.task_id)?
                        != work
                            .raw_before
                            .as_ref()
                            .map(serde_json::to_value)
                            .transpose()?
                {
                    return Err(Error::State("task or transfer changed during relocation"));
                }
                tx.put(
                    &self.owner,
                    TASKS,
                    &op.task_id,
                    &serde_json::to_value(after)?,
                )?;
                if let Some(raw) = raw {
                    tx.put(&self.owner, RAW, &op.task_id, &serde_json::to_value(raw)?)?;
                }
                if let Some(f) = audio {
                    relocate_download(
                        tx,
                        &f.source,
                        Some((
                            f.destination.as_deref().expect("target"),
                            new_fp.expect("fingerprint"),
                        )),
                        Some(&f.expected.fingerprint),
                    )?;
                }
                for row in tx.rows("offline_music_v1")? {
                    if row.owner != self.owner {
                        continue;
                    }
                    let mut record: OfflineRecord = serde_json::from_value(row.value)?;
                    if record.task_id != op.task_id {
                        continue;
                    }
                    let target = op
                        .target
                        .as_ref()
                        .ok_or(Error::State("offline task lost target"))?;
                    record.path = target.clone();
                    record.id = fs::path_key(target)?;
                    record.revision = uuid::Uuid::new_v4().to_string();
                    if tx
                        .get(&self.owner, "offline_music_v1", &record.id)?
                        .is_some()
                    {
                        return Err(Error::State("destination offline record already exists"));
                    }
                    tx.put(
                        &self.owner,
                        "offline_music_v1",
                        &record.id,
                        &serde_json::to_value(&record)?,
                    )?;
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
    pub(in crate::download::completed) fn cleanup_work(
        &self,
        op: &mut Operation,
        token: &Option<SessionToken>,
        cancelling: bool,
    ) -> Result<()> {
        let Some(work) = &mut op.working else {
            return Ok(());
        };
        let (path, expected, parent) = if cancelling {
            (
                &work.after.work,
                &work.new_work_identity,
                &work.after.root_identity,
            )
        } else {
            (
                &op.task.work,
                &op.task.work_identity,
                &op.task.root_identity,
            )
        };
        let retained = self.guarded(token, || {
            match stdfs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                r => {
                    r?;
                }
            }
            fs::parent(path, parent)?;
            if !stdfs::symlink_metadata(path)?.is_dir() || identity(path)? != *expected {
                return Ok(true);
            }
            match stdfs::remove_dir(path) {
                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(true),
                r => {
                    r?;
                    fs::sync(path.parent().expect("parent"))?;
                    Ok(false)
                }
            }
        })?;
        if !cancelling {
            work.old_work_retained = retained;
        }
        self.save(op)
    }
}
