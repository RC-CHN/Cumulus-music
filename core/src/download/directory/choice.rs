//! Preview an explicitly picked directory before admitting a durable move.
use super::*;

impl DownloadDirectory {
    pub fn preview_choice(&self, target: &Path) -> Result<DirectoryChoice> {
        let token = self.files.token()?;
        let before = self.choice_settings(&token)?;
        let choice = self.check_choice(target, before.as_ref(), &token, None)?;
        // A concurrent settings change also retires the displayed decision.
        if self.choice_settings(&token)? != before {
            return Err(Error::StaleResponse);
        }
        Ok(choice)
    }

    pub fn change_from_choice(&self, selected: &DirectoryChoice) -> Result<Option<MigrationView>> {
        use DirectoryChoiceDecision::*;
        let expected = selected
            .target_identity
            .as_ref()
            .ok_or(Error::StaleResponse)?;
        if selected.owner != self.files.owner || selected.decision != Ready {
            return Err(Error::StaleResponse);
        }
        let token = self.files.token()?;
        let _directory = gate::lock(&self.files.store, &self.files.owner)?;
        let before = self.choice_settings(&token)?;
        if before.as_ref().map(|value| &value.revision) != selected.revision.as_ref() {
            return Err(Error::StaleResponse);
        }
        let choice =
            self.check_choice(&selected.target, before.as_ref(), &token, Some(expected))?;
        match choice.decision {
            SameDirectory => Err(Error::InvalidInput("choose a different download directory")),
            NotWritable => Err(Error::InvalidInput("download directory is not writable")),
            InsufficientSpace => Err(Error::State("insufficient space for completed downloads")),
            Ready if before.is_some() => self
                .start_locked(&choice.target, None, Some(expected), &token)
                .map(Some),
            Ready => {
                self.configure_locked(&choice.target, 0, 1, Some(expected), &token)?;
                Ok(None)
            }
        }
    }

    fn choice_settings(&self, token: &Option<SessionToken>) -> Result<Option<DirectorySettings>> {
        self.files.guarded(token, || {
            gate::require(&self.files.store, &self.files.owner, None)?;
            self.read_settings()
        })
    }

    fn check_choice(
        &self,
        target: &Path,
        before: Option<&DirectorySettings>,
        token: &Option<SessionToken>,
        expected: Option<&Identity>,
    ) -> Result<DirectoryChoice> {
        use DirectoryChoiceDecision::*;
        let result = |target, decision| DirectoryChoice {
            target,
            revision: before.map(|value| value.revision.clone()),
            decision,
            move_existing: before.is_some(),
            owner: self.files.owner.clone(),
            target_identity: None,
        };
        if !target.is_absolute() || target.as_os_str().as_encoded_bytes().len() > 32 * 1024 {
            return Err(Error::InvalidInput("expected a picked absolute directory"));
        }
        if before.is_some_and(|value| value.root == target) {
            return Ok(result(target.to_path_buf(), SameDirectory));
        }
        let selected = target;
        let target = match paths::root(selected, self.files.store.directory(), false) {
            Ok(path) => path,
            Err(Error::Io(_) | Error::InvalidInput(_)) => {
                return Ok(result(target.to_path_buf(), NotWritable));
            }
            Err(error) => return Err(error),
        };
        if let Some(expected) = expected {
            check_choice_identity(selected, &target, expected)?;
        }
        let current = identity(&target)?;
        if let Some(before) = before {
            if before.root == target {
                return Ok(result(target, SameDirectory));
            }
            if before.root.starts_with(&target) || target.starts_with(&before.root) {
                return Ok(result(target, NotWritable));
            }
        }
        self.files.guarded(token, || Ok(()))?;
        if paths::writable(&target).is_err() {
            return Ok(result(target, NotWritable));
        }
        if before.is_some() {
            let available = match paths::available(&target) {
                Ok(bytes) => bytes,
                Err(Error::Io(_)) => return Ok(result(target, NotWritable)),
                Err(error) => return Err(error),
            };
            let rows = self
                .files
                .guarded(token, || self.files.store.values(&self.files.owner, TASKS))?;
            let mut total = 0u64;
            for row in rows.into_values() {
                let task: DownloadTask = serde_json::from_value(row)?;
                if task.status == DownloadStatus::Complete {
                    // The downloaded output can differ from raw transfer bytes.
                    let bytes = task
                        .output_integrity
                        .map_or(task.bytes, |value| value.bytes);
                    let Some(next) = total.checked_add(bytes) else {
                        return Ok(result(target, InsufficientSpace));
                    };
                    total = next;
                }
            }
            if total > available {
                return Ok(result(target, InsufficientSpace));
            }
        }
        check_choice_identity(&target, &target, &current)?;
        let mut choice = result(target, Ready);
        choice.target_identity = Some(current);
        Ok(choice)
    }
}
