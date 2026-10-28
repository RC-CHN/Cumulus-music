use super::*;

/// A cancelled future is a lost acknowledgement, not permission to replay.
/// Disk/process failure is recovered from the durable Sending phase on reopen.
struct Flight<'a> {
    publisher: &'a RelayPublisher,
    owner: &'a str,
    row: PublicationIntent,
}
impl Drop for Flight<'_> {
    fn drop(&mut self) {
        let _ = self
            .publisher
            .finish(self.owner, &self.row, PublicationPhase::Uncertain, None);
    }
}
impl RelayPublisher {
    fn finish(
        &self,
        owner: &str,
        row: &PublicationIntent,
        phase: PublicationPhase,
        code: Option<i64>,
    ) -> Result<()> {
        self.store.transaction(|tx| {
            let mut current: PublicationIntent = serde_json::from_value(
                tx.get(owner, JOURNAL, &row.id)?
                    .ok_or(Error::State("relay publication intent missing"))?,
            )?;
            if current.phase != PublicationPhase::Sending {
                return Ok(());
            }
            if current.device_id != row.device_id {
                return Err(Error::State("relay publication intent device mismatch"));
            }
            current.phase = phase;
            current.service_code = code;
            let mut state = load(tx, owner, &row.device_id)?;
            if matches!(
                phase,
                PublicationPhase::Rejected | PublicationPhase::Uncertain
            ) {
                state.blocked = Some(row.id.clone());
                state.recovery = None;
            } else if phase == PublicationPhase::Confirmed
                && state.recovery.as_deref() == Some(row.id.as_str())
            {
                state.blocked = None;
                state.recovery = None;
            }
            put(tx, owner, &current)?;
            save(tx, owner, &state)
        })
    }
    /// Fresh native preflight on every dispatch; a saved allow flag never
    /// authorizes a restarted publisher. RELAY_SWITCH is not an extra upload
    /// gate in M/401: shouldUpload is authenticated && t2 && config.enable.
    async fn preflight(&self, token: &SessionToken, device: &str) -> Result<Option<RelayConfig>> {
        if RelaySettings::new(self.session.clone())?
            .operations_for(token)?
            .iter()
            .any(|op| op.phase.unresolved())
        {
            return Err(Error::MutationUncertain);
        }
        let result = self
            .session
            .run_for(token, |api, _| async move {
                if api.local_device_id()?.as_deref() != Some(device) {
                    return Err(Error::StaleResponse);
                }
                let relay = RelayApi::new(api.clone());
                if !relay.experiment().await? {
                    return Ok(None);
                }
                let config = relay.config().await?;
                if api.local_device_id()?.as_deref() != Some(device) {
                    return Err(Error::StaleResponse);
                }
                Ok(config.enable.then_some(config))
            })
            .await;
        // This cache controls only native sampling, never HTTP eligibility.
        // Clearing it during each successful preflight would restart the voice
        // clock after its own upload and accidentally remove the throttle.
        *self.config.lock().map_err(|_| Error::LockPoisoned)? = result
            .as_ref()
            .ok()
            .and_then(|value| value.as_ref())
            .map(|config| (token.clone(), device.into(), config.clone()));
        let config = result?;
        Ok(config)
    }
    fn next(
        &self,
        owner: &str,
        device: &str,
        config: &RelayConfig,
    ) -> Result<Option<PublicationIntent>> {
        self.store.transaction(|tx| {
            let mut state = load(tx, owner, device)?;
            if state.blocked.is_some() && state.recovery.is_none() {
                return Err(Error::MutationUncertain);
            }
            if state.needs_snapshot
                && state.recovery.is_none()
                && state
                    .frame
                    .as_ref()
                    .is_some_and(PublicationFrame::can_publish)
            {
                let frame = state
                    .frame
                    .clone()
                    .ok_or(Error::State("relay queue missing"))?;
                record::supersede(tx, owner, device, None)?;
                let upload = record::snapshot(&mut state, &frame, config)?;
                append(tx, owner, &mut state, upload, None)?;
                state.needs_snapshot = false;
            }
            let mut row = records(tx, owner, device)?.into_iter().find(|row| {
                row.phase == PublicationPhase::Prepared
                    && state.recovery.as_deref().is_none_or(|id| id == row.id)
            });
            if let Some(row) = &mut row {
                row.phase = PublicationPhase::Sending;
                put(tx, owner, row)?;
            }
            save(tx, owner, &state)?;
            Ok(row)
        })
    }
    async fn dispatch(
        &self,
        token: &SessionToken,
        owner: &str,
        row: &PublicationIntent,
    ) -> Result<()> {
        let _flight = Flight {
            publisher: self,
            owner,
            row: row.clone(),
        };
        let result = self
            .session
            .run_for(token, |api, _| async move {
                if api.local_device_id()?.as_deref() != Some(row.device_id.as_str()) {
                    return Err(Error::StaleResponse);
                }
                RelayApi::new(api).submit(&row.upload).await
            })
            .await;
        let (phase, code) = match &result {
            Ok(()) => (PublicationPhase::Confirmed, None),
            Err(error) => {
                let rejected = matches!(error.root(), Error::RequestNotSent(_))
                    || [301, 403, 404, 505, 506, 511, 512, 10001]
                        .iter()
                        .any(|c| error.is_service_code(*c));
                let code = match error.root() {
                    Error::Service { code, .. } => code
                        .as_i64()
                        .or_else(|| code.as_str().and_then(|s| s.parse().ok())),
                    _ => None,
                };
                (
                    if rejected {
                        PublicationPhase::Rejected
                    } else {
                        PublicationPhase::Uncertain
                    },
                    code,
                )
            }
        };
        // Persist to the original account even if the session changed in flight.
        self.finish(owner, row, phase, code)?;
        result
    }
    fn fresh_snapshot(
        &self,
        token: &SessionToken,
        owner: &str,
        device: &str,
        original: &str,
        id: Option<&str>,
        config: &RelayConfig,
    ) -> Result<PublicationIntent> {
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let mut state = load(tx, owner, device)?;
                if state.blocked.as_deref() != Some(original) {
                    return Err(Error::StaleResponse);
                }
                if state.recovery.is_some() {
                    return Err(Error::MutationBusy);
                }
                let frame = state
                    .frame
                    .clone()
                    .ok_or(Error::State("relay queue missing"))?;
                // Read the LATEST committed queue here, after the failed request;
                // commands arriving during HTTP must not restore an obsolete queue.
                let upload = record::snapshot(&mut state, &frame, config)?;
                record::supersede(tx, owner, device, None)?;
                let row = append_named(
                    tx,
                    owner,
                    &mut state,
                    upload,
                    Some(original.into()),
                    id.map(str::to_owned)
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                )?;
                state.recovery = Some(row.id.clone());
                state.needs_snapshot = false;
                save(tx, owner, &state)?;
                Ok(row)
            })
        })
    }
    /// One normal dispatch, optionally followed by exactly ONE snapshot after
    /// an explicit 10001. Definite rejections and unknown writes block later
    /// cumulative commands; neither failure type silently replays old changes.
    pub async fn publish_next_for(&self, token: &SessionToken) -> Result<PublishOutcome> {
        let relay = RelayService::new(self.session.clone())?;
        let (owner, _lock) = relay.lock(token)?;
        let (_, device) = self.identity(token)?;
        self.recover(&owner, &device)?;
        let status = self.status_for(token)?;
        if let Some(blocked) = status.blocked
            && status.recovering.is_none()
        {
            return Ok(PublishOutcome::Blocked(blocked.id));
        }
        if status.pending == 0 {
            return Ok(PublishOutcome::Idle);
        }
        let Some(config) = self.preflight(token, &device).await? else {
            self.session.with_token(token, |_| {
                self.store.transaction(|tx| {
                    let mut state = load(tx, &owner, &device)?;
                    for mut row in records(tx, &owner, &device)? {
                        if row.phase == PublicationPhase::Prepared {
                            row.phase = PublicationPhase::Skipped;
                            put(tx, &owner, &row)?;
                        }
                    }
                    state.needs_snapshot = true;
                    state.recovery = None;
                    save(tx, &owner, &state)
                })
            })?;
            return Ok(PublishOutcome::Skipped);
        };
        let row = self
            .session
            .with_token(token, |_| self.next(&owner, &device, &config))?
            .ok_or(Error::State("relay publication queue changed"))?;
        let result = self.dispatch(token, &owner, &row).await;
        self.check_identity(token, &owner, &device)?;
        match result {
            Ok(()) => Ok(PublishOutcome::Confirmed(row.id)),
            Err(error) => {
                self.session.check_generation(token)?;
                if row.upload.requires_snapshot(&error) {
                    // An excluded/empty queue cannot synthesize a valid snapshot.
                    // Retain the rejected responsibility instead of resending clear.
                    if !self.status_for(token)?.can_resynchronize {
                        return Ok(PublishOutcome::Blocked(row.id));
                    }
                    let recovery =
                        self.fresh_snapshot(token, &owner, &device, &row.id, None, &config)?;
                    let next = self
                        .session
                        .with_token(token, |_| self.next(&owner, &device, &config))?
                        .ok_or(Error::State("relay recovery missing"))?;
                    if next.id != recovery.id {
                        return Err(Error::State("relay recovery ordering changed"));
                    }
                    let result = self.dispatch(token, &owner, &next).await;
                    self.check_identity(token, &owner, &device)?;
                    match result {
                        Ok(()) => Ok(PublishOutcome::Recovered {
                            original: row.id,
                            snapshot: next.id,
                        }),
                        Err(_) => {
                            self.session.check_generation(token)?;
                            Ok(PublishOutcome::Blocked(next.id))
                        }
                    }
                } else {
                    Ok(PublishOutcome::Blocked(row.id))
                }
            }
        }
    }
    /// Explicit user recovery starts a new session from the current queue. The
    /// original rejected/uncertain receipt remains intact. Duplicate request IDs
    /// read their existing intent and never generate another recovery snapshot.
    pub async fn resynchronize_for(
        &self,
        token: &SessionToken,
        id: &str,
        blocked: &str,
    ) -> Result<PublicationIntent> {
        super::super::validate_id(id)?;
        super::super::validate_id(blocked)?;
        let relay = RelayService::new(self.session.clone())?;
        let (owner, _lock) = relay.lock(token)?;
        let (_, device) = self.identity(token)?;
        self.recover(&owner, &device)?;
        if let Some(raw) = self.store.get(&owner, JOURNAL, id)? {
            let row: PublicationIntent = serde_json::from_value(raw)?;
            if row.id != id
                || row.device_id != device
                || row.recovery_of.as_deref() != Some(blocked)
            {
                return Err(Error::InvalidInput("relay recovery intent reused"));
            }
            self.session.check_generation(token)?;
            return Ok(row);
        }
        let config = self
            .preflight(token, &device)
            .await?
            .ok_or(Error::State("relay publishing unavailable"))?;
        self.fresh_snapshot(token, &owner, &device, blocked, Some(id), &config)
    }
}
