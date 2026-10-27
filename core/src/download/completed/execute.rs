use super::*;
use crate::download::file_identity::identity;
use std::{
    fs as stdfs,
    io::{Read, Seek, Write},
};

impl CompletedDownloads {
    pub fn resume(&self, id: &str) -> Result<FileOperationView> {
        let token = self.token()?;
        self.guard(&token)?;
        let task = self.read(id)?.task_id;
        let _locks = self.locks(&task)?;
        let mut op = self.read(id)?;
        if op.migration_id.as_deref() != self.migration.as_ref().map(|m| m.0.as_str()) {
            return Err(Error::State(
                "file operation belongs to a directory migration",
            ));
        }
        if matches!(op.phase, FilePhase::Complete | FilePhase::Cancelled) {
            return Ok(op.view());
        }
        let _playback = self.wait_for_playback(&op, &token)?;
        let result = self.advance(&mut op, &token);
        if result.is_err() {
            op.error = Some("file_operation_conflict_or_interruption".into());
            self.save(&op)?;
        }
        result?;
        self.guard(&token)?;
        Ok(op.view())
    }
    fn wait_for_playback(
        &self,
        op: &Operation,
        token: &Option<SessionToken>,
    ) -> Result<OperationLock> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            self.guard(token)?;
            match crate::download::playback::lock(&self.store, &self.owner, &op.task_id) {
                Ok(lease) => return Ok(lease),
                Err(Error::MutationBusy) if std::time::Instant::now() < deadline => {
                    // Also gives hosts a cancellation point while the decoder
                    // drains. No account or database mutex is held here.
                    self.event(FileEvent::WaitingForPlayback, op)?;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }
    fn advance(&self, op: &mut Operation, token: &Option<SessionToken>) -> Result<()> {
        if op.phase == FilePhase::Prepared {
            for i in 0..op.files.len() {
                let f = &op.files[i];
                match current_source(f) {
                    Err(Error::Io(e))
                        if e.kind() == std::io::ErrorKind::NotFound
                            && op.action == FileAction::Delete => {}
                    r => r?,
                }
                if op.action == FileAction::Move {
                    self.destination(op, i, token)?;
                }
            }
            self.commit(op, token)?;
            self.event(FileEvent::MetadataCommitted, op)?;
        }
        for i in 0..op.files.len() {
            if op.action == FileAction::Move {
                current_destination(&op.files[i])?;
            }
            if op.files[i].stage != FileStage::Cleaned {
                self.unlink_source(&mut op.files[i], token)?;
                op.files[i].stage = FileStage::Cleaned;
                self.save(op)?;
                self.event(FileEvent::SourceRemoved, op)?;
            }
            self.clear_stage(&op.files[i], token)?;
        }
        self.cleanup_work(op, token, false)?;
        self.finish(op, token, FilePhase::Complete)?;
        self.event(FileEvent::Finished, op)
    }
    fn destination(
        &self,
        op: &mut Operation,
        i: usize,
        token: &Option<SessionToken>,
    ) -> Result<()> {
        if op.files[i].stage == FileStage::Ready {
            return current_destination(&op.files[i]);
        }
        current_source(&op.files[i])?;
        let destination = op.files[i].destination.clone().expect("move destination");
        fs::parent(
            &destination,
            op.files[i].destination_parent.as_ref().expect("parent"),
        )?;
        if let Some(alias) = op.files[i].alias {
            if alias >= i {
                return Err(Error::State("invalid hard-link alias order"));
            }
            current_destination(&op.files[alias])?;
            op.files[i].destination_facts = op.files[alias].destination_facts.clone();
            self.save(op)?;
            if stdfs::symlink_metadata(&destination).is_ok() {
                current_destination(&op.files[i])?;
            } else {
                self.guarded(token, || {
                    let first = &op.files[alias];
                    let path = first.destination.as_deref().expect("alias");
                    fs::parent(path, first.destination_parent.as_ref().expect("parent"))?;
                    fs::unchanged(path, first.destination_facts.as_ref().expect("ready"))?;
                    fs::parent(
                        &destination,
                        op.files[i].destination_parent.as_ref().expect("parent"),
                    )?;
                    stdfs::hard_link(
                        op.files[alias].destination.as_ref().expect("alias"),
                        &destination,
                    )?;
                    fs::sync(destination.parent().expect("parent"))
                })?;
            }
            current_destination(&op.files[i])?;
            op.files[i].stage = FileStage::Ready;
            self.save(op)?;
            return self.event(FileEvent::DestinationReady, op);
        }
        if op.files[i].destination_facts.is_none() {
            op.files[i].destination_facts = Some(op.files[i].expected.clone());
            self.save(op)?;
        }
        if stdfs::symlink_metadata(&destination).is_ok() {
            current_destination(&op.files[i])?;
        } else if self.options.copy_only || op.files[i].temporary.is_some() {
            self.copy(op, i, token)?;
        } else {
            let result = self.guarded(token, || {
                fs::parent(&op.files[i].source, &op.files[i].source_parent)?;
                fs::unchanged(&op.files[i].source, &op.files[i].expected)?;
                fs::parent(
                    &destination,
                    op.files[i].destination_parent.as_ref().expect("parent"),
                )?;
                stdfs::hard_link(&op.files[i].source, &destination).map_err(Error::from)
            });
            match result {
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                    self.copy(op, i, token)?
                }
                r => r?,
            }
        }
        fs::sync(destination.parent().expect("parent"))?;
        current_destination(&op.files[i])?;
        op.files[i].stage = FileStage::Ready;
        self.save(op)?;
        self.event(FileEvent::DestinationReady, op)
    }
    fn copy(&self, op: &mut Operation, i: usize, token: &Option<SessionToken>) -> Result<()> {
        let dest = op.files[i].destination.clone().expect("destination");
        if op.files[i].temporary.is_none() {
            self.guard(token)?;
            let temp = tempfile::Builder::new()
                .prefix(".cumulus-move-")
                .tempfile_in(dest.parent().expect("parent"))?;
            let (handle, path) = temp.keep().map_err(|e| Error::Io(e.error))?;
            op.files[i].temporary_identity = Some(identity(&path)?);
            op.files[i].temporary = Some(path);
            handle.sync_all()?;
            if let Err(e) = self.save(op) {
                let _ = self.clear_stage(&op.files[i], token);
                return Err(e);
            }
        }
        let f = &op.files[i];
        let stage = f.temporary.clone().expect("stage");
        stage_identity(f)?;
        current_source(f)?;
        let mut source = crate::media::open_regular(&f.source)?;
        let mut output = stdfs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&stage)?;
        if same_file::Handle::from_file(output.try_clone()?)?
            != same_file::Handle::from_path(&stage)?
            || same_file::Handle::from_file(source.try_clone()?)?
                != same_file::Handle::from_path(&f.source)?
        {
            return Err(Error::State("move file changed while opening"));
        }
        output.set_len(0)?;
        output.rewind()?;
        op.files[i].stage = FileStage::Copying;
        op.files[i].bytes_copied = 0;
        self.save(op)?;
        let mut block = vec![0; 1024 * 1024];
        loop {
            self.guard(token)?;
            let n = source.read(&mut block)?;
            if n == 0 {
                break;
            }
            output.write_all(&block[..n])?;
            op.files[i].bytes_copied += n as u64;
            self.save(op)?;
            self.event(FileEvent::CopyChunk, op)?;
        }
        let expected = &op.files[i].expected;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = expected.mode {
                output.set_permissions(stdfs::Permissions::from_mode(mode))?;
            }
        }
        #[cfg(not(unix))]
        {
            let mut p = output.metadata()?.permissions();
            p.set_readonly(expected.readonly);
            output.set_permissions(p)?;
        }
        output.set_times(stdfs::FileTimes::new().set_modified(expected.modified))?;
        output.sync_all()?;
        current_source(&op.files[i])?;
        let facts = fs::facts(&stage)?;
        if facts.integrity != expected.integrity {
            return Err(Error::State("copied output failed integrity check"));
        }
        op.files[i].destination_facts = Some(facts);
        self.save(op)?;
        self.event(FileEvent::Staged, op)?;
        self.guarded(token, || {
            stage_identity(&op.files[i])?;
            fs::parent(
                &dest,
                op.files[i].destination_parent.as_ref().expect("parent"),
            )?;
            stdfs::hard_link(&stage, &dest)?;
            fs::sync(dest.parent().expect("parent"))
        })
    }
    fn unlink_source(&self, f: &mut ManagedFile, token: &Option<SessionToken>) -> Result<()> {
        // Hash outside the account lock. Only final identity/stat/unlink are
        // guarded so an account change can interrupt long file verification.
        if let Ok(meta) = stdfs::symlink_metadata(&f.source)
            && meta.is_file()
            && identity(&f.source)? == f.expected.identity
        {
            current_source(f)?;
        }
        self.guarded(token, || {
            let meta = match stdfs::symlink_metadata(&f.source) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                r => r?,
            };
            fs::parent(&f.source, &f.source_parent)?;
            if !meta.is_file() || identity(&f.source)? != f.expected.identity {
                f.replacement_retained = true;
                return Ok(());
            }
            fs::unchanged(&f.source, &f.expected)?;
            stdfs::remove_file(&f.source)?;
            fs::sync(f.source.parent().expect("parent"))
        })
    }
    fn clear_stage(&self, f: &ManagedFile, token: &Option<SessionToken>) -> Result<()> {
        let Some(path) = &f.temporary else {
            return Ok(());
        };
        self.guarded(token, || {
            match stdfs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                r => {
                    r?;
                }
            }
            stage_identity(f)?;
            stdfs::remove_file(path)?;
            fs::sync(path.parent().expect("parent"))
        })
    }
    pub fn cancel(&self, id: &str) -> Result<FileOperationView> {
        let token = self.token()?;
        self.guard(&token)?;
        let _locks = self.locks(&self.read(id)?.task_id)?;
        let mut op = self.read(id)?;
        if op.migration_id.as_deref() != self.migration.as_ref().map(|m| m.0.as_str()) {
            return Err(Error::State(
                "file operation belongs to a directory migration",
            ));
        }
        if op.phase == FilePhase::Cancelled {
            return Ok(op.view());
        }
        if op.phase != FilePhase::Prepared {
            return Err(Error::State("metadata committed; finish cleanup instead"));
        }
        if op.action == FileAction::Move {
            for f in &op.files {
                current_source(f)?;
            }
        }
        for f in &op.files {
            if let Some(dest) = &f.destination {
                match stdfs::symlink_metadata(dest) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                    Ok(_) => current_destination(f)?,
                }
                self.guarded(&token, || {
                    match stdfs::symlink_metadata(dest) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                        r => {
                            r?;
                        }
                    }
                    fs::parent(dest, f.destination_parent.as_ref().expect("parent"))?;
                    fs::unchanged(
                        dest,
                        f.destination_facts
                            .as_ref()
                            .ok_or(Error::State("unowned destination"))?,
                    )?;
                    stdfs::remove_file(dest)?;
                    fs::sync(dest.parent().expect("parent"))
                })?;
            }
            self.clear_stage(f, &token)?;
        }
        self.cleanup_work(&mut op, &token, true)?;
        self.finish(&mut op, &token, FilePhase::Cancelled)?;
        Ok(op.view())
    }
}
pub(super) fn current_source(f: &ManagedFile) -> Result<()> {
    fs::parent(&f.source, &f.source_parent)?;
    fs::matches(&f.source, &f.expected)
}
pub(super) fn current_destination(f: &ManagedFile) -> Result<()> {
    let dest = f
        .destination
        .as_ref()
        .ok_or(Error::State("missing destination"))?;
    fs::parent(
        dest,
        f.destination_parent
            .as_ref()
            .ok_or(Error::State("missing destination parent"))?,
    )?;
    fs::matches(
        dest,
        f.destination_facts
            .as_ref()
            .ok_or(Error::State("unowned destination"))?,
    )
}
fn stage_identity(f: &ManagedFile) -> Result<()> {
    let path = f
        .temporary
        .as_ref()
        .ok_or(Error::State("missing staging file"))?;
    fs::parent(
        path,
        f.destination_parent
            .as_ref()
            .ok_or(Error::State("missing destination parent"))?,
    )?;
    if !stdfs::symlink_metadata(path)?.is_file() || Some(identity(path)?) != f.temporary_identity {
        return Err(Error::State("move staging file was replaced"));
    }
    Ok(())
}
