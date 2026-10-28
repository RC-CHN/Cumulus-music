use super::*;
impl Inner {
    pub(super) async fn filter(
        &self,
        token: &SessionToken,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let (view, actual, desired) = self.current(token).await?;
        self.guard(token, intent)?;
        Self::require_synced(&desired, &actual)?;
        let before = desired.current().map(|t| t.entry_id.clone());
        let filtered = self
            .service
            .playable_for(token, &desired.source, desired.revision)
            .await?;
        let changed = filtered.current().map(|t| t.entry_id.clone()) != before;
        self.apply(
            token,
            &view,
            &filtered,
            intent,
            changed.then_some(QueueTransport {
                position: Some(0.0),
                paused: !matches!(
                    view.state,
                    PlaybackStatus::Playing | PlaybackStatus::Buffering
                ),
            }),
        )
        .await
    }
    pub(super) async fn step(
        &self,
        token: &SessionToken,
        step: FmStep,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        self.guard(token, intent)?;
        let (before, actual, desired) = self.current(token).await?;
        Self::require_synced(&desired, &actual)?;
        let transition = self
            .service
            .step_for(
                token,
                &desired.source,
                desired.revision,
                step,
                observation(&before).as_ref(),
            )
            .await?;
        if let Err(error) = self.guard(token, intent) {
            if let Some(id) = &transition.skip_report {
                let _ = self.service.abandon_skip_for(token, id);
            }
            return Err(error);
        }
        if let Some(id) = transition.skip_report {
            let service = self.service.clone();
            let token = token.clone();
            // Captured time/algorithm are immutable. Reporting never blocks
            // source resolution or audio; the journal retains unknown results.
            tokio::spawn(async move {
                let _ = service.dispatch_skip_for(&token, &id).await;
            });
        }
        let result = self
            .apply(
                token,
                &before,
                &transition.queue,
                intent,
                transition.play.then_some(QueueTransport {
                    position: Some(0.0),
                    paused: false,
                }),
            )
            .await;
        if result.is_ok() && transition.refill_failed {
            self.failure(token, Some("refill_failed"))?;
        }
        result
    }
    pub(super) async fn resume(
        &self,
        token: &SessionToken,
        restore: bool,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let (view, actual, mut desired) = self.current(token).await?;
        self.guard(token, intent)?;
        if matches_queue(&desired, &actual) && desired.current().is_some() {
            return self
                .controller
                .command(
                    token,
                    if restore {
                        PlaybackCommand::Restore
                    } else {
                        PlaybackCommand::Play
                    },
                )
                .await;
        }
        if desired.current().is_none() {
            let rights = crate::membership::MembershipManager::new(self.session.clone())?
                .refresh_for(token.clone())
                .await?;
            desired = self
                .service
                .preload_for(token, desired.source.clone(), FmRights::from(&rights))
                .await?;
        }
        self.apply(
            token,
            &view,
            &desired,
            intent,
            Some(QueueTransport {
                position: Some(0.0),
                paused: restore,
            }),
        )
        .await
    }
    pub(super) async fn startup(
        &self,
        token: &SessionToken,
        mut input: crate::playback::controller::StartupRestore,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let (view, actual, mut desired) = self.current(token).await?;
        self.guard(token, intent)?;
        if !view.startup_pending {
            return Ok(view);
        }
        if view.fence() != input.expected {
            return Err(Error::QueueChanged);
        }
        if !matches_queue(&desired, &actual) || desired.current().is_none() {
            if desired.current().is_none() {
                let rights = crate::membership::MembershipManager::new(self.session.clone())?
                    .refresh_for(token.clone())
                    .await?;
                desired = self
                    .service
                    .preload_for(token, desired.source.clone(), FmRights::from(&rights))
                    .await?;
            }
            input.queue = Some(projection(&desired, quality(&view))?);
        }
        self.guard(token, intent)?;
        self.controller
            .command(token, PlaybackCommand::Startup(input))
            .await
    }
    pub(super) async fn repeat(
        &self,
        token: &SessionToken,
        repeat: FmRepeat,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let (view, actual, desired) = self.current(token).await?;
        self.guard(token, intent)?;
        Self::require_synced(&desired, &actual)?;
        let desired = self
            .service
            .repeat_for(token, &desired.source, desired.revision, repeat)?;
        self.apply(token, &view, &desired, intent, None).await
    }
    pub(super) async fn trash(
        &self,
        token: &SessionToken,
        entry_id: &str,
        reason_key: Option<&str>,
        intent: u64,
    ) -> Result<FmTrashMutation> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let (view, actual, desired) = self.current(token).await?;
        self.guard(token, intent)?;
        if view
            .selected
            .as_ref()
            .is_none_or(|e| e.entry_id != entry_id)
        {
            return Err(Error::QueueChanged);
        }
        Self::require_synced(&desired, &actual)?;
        let result = self
            .service
            .trash_reason_for(
                token,
                &desired.source,
                desired.revision,
                observation(&view).as_ref(),
                reason_key,
            )
            .await?;
        self.guard(token, intent)?;
        // Official order: song trash, optional artist blacklist, then advance.
        // Artist failure remains visible but does not undo the confirmed song.
        let artist_result = self.service.finish_trash_artist_for(token, &result).await;
        self.guard(token, intent)?;
        let transition = self
            .service
            .apply_trash_for(token, &desired.source, desired.revision, &result.id)
            .await?;
        self.apply(
            token,
            &view,
            &transition.queue,
            intent,
            transition.play.then_some(QueueTransport {
                position: Some(0.0),
                paused: false,
            }),
        )
        .await?;
        artist_result?;
        Ok(result)
    }
    pub(super) async fn apply_trash(
        &self,
        token: &SessionToken,
        id: &str,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let (view, actual, desired) = self.current(token).await?;
        self.guard(token, intent)?;
        Self::require_synced(&desired, &actual)?;
        let record = self.service.reconcile_trash_for(token, id).await?;
        if !record.desired {
            return Ok(view);
        }
        let artist_result = self.service.finish_trash_artist_for(token, &record).await;
        self.guard(token, intent)?;
        let transition = self
            .service
            .apply_trash_for(token, &desired.source, desired.revision, id)
            .await?;
        let applied = self
            .apply(
                token,
                &view,
                &transition.queue,
                intent,
                transition.play.then_some(QueueTransport {
                    position: Some(0.0),
                    paused: false,
                }),
            )
            .await?;
        artist_result?;
        Ok(applied)
    }
}
