use super::*;
use crate::playback::{FM_BACKUP_POSITION, FM_BACKUP_QUEUE};
use serde_json::json;
impl Inner {
    pub(super) fn active_source(
        &self,
        token: &SessionToken,
        queue: &QueueSnapshot,
    ) -> Result<Option<FmSource>> {
        self.session.with_token(token, |uid| {
            let actual = source(queue)?;
            if !queue.items.is_empty() || actual.is_some() {
                return Ok(actual);
            }
            let saved = self
                .store
                .get(uid, "fm-player-v1", "source")?
                .map(serde_json::from_value::<FmSource>)
                .transpose()?;
            if let Some(value) = &saved {
                value.validate()?;
            }
            Ok(saved)
        })
    }
    pub(super) fn guard(&self, token: &SessionToken, intent: u64) -> Result<()> {
        self.session.with_token(token, |_| Ok(()))?;
        if self.intents.load(Ordering::Acquire) != intent {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    pub(super) fn failure(&self, token: &SessionToken, code: Option<&str>) -> Result<()> {
        self.session.with_token(token, |uid| match code {
            Some(code) => self
                .store
                .put(uid, "fm-player-v1", "failure", &json!({"code":code})),
            None => self.store.delete(uid, "fm-player-v1", "failure"),
        })
    }
    pub(super) fn backup(
        &self,
        token: &SessionToken,
        view: &PlaybackSnapshot,
        queue: &QueueSnapshot,
    ) -> Result<()> {
        let mut queue = queue.clone();
        if queue
            .intelligence
            .as_ref()
            .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
        {
            return Err(Error::State("leave intelligence before entering FM"));
        }
        // Fallback is ordinary playback. Its consumed notice must not become
        // a dangling coordinator reference in the FM return snapshot.
        queue.intelligence = None;
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                tx.put(
                    uid,
                    "playback",
                    FM_BACKUP_QUEUE,
                    &serde_json::to_value(&queue)?,
                )?;
                tx.put(
                    uid,
                    "playback",
                    FM_BACKUP_POSITION,
                    &json!({"entry_id":queue.current,"position":view.position}),
                )
            })
        })
    }
    pub(super) async fn apply(
        &self,
        token: &SessionToken,
        before: &PlaybackSnapshot,
        queue: &FmQueue,
        intent: u64,
        transport: Option<QueueTransport>,
    ) -> Result<PlaybackSnapshot> {
        self.guard(token, intent)?;
        let result = self
            .controller
            .command(
                token,
                PlaybackCommand::UpdateQueue(QueueUpdate {
                    expected: before.fence(),
                    queue: projection(queue, quality(before))?,
                    transport,
                }),
            )
            .await;
        if self.session.check_generation(token).is_ok() {
            self.failure(
                token,
                if result.is_err() {
                    Some("apply_failed")
                } else {
                    None
                },
            )?;
        }
        result
    }
    pub(super) async fn current(
        &self,
        token: &SessionToken,
    ) -> Result<(PlaybackSnapshot, QueueSnapshot, FmQueue)> {
        let (view, actual) = self.controller.queue_with_state(token).await?;
        let source = self
            .active_source(token, &actual)?
            .ok_or(Error::State("FM is not the current queue"))?;
        let desired = self
            .service
            .queue_for(token, &source)?
            .ok_or(Error::State("FM cache is missing"))?;
        Ok((view, actual, desired))
    }
    pub(super) fn require_synced(desired: &FmQueue, actual: &QueueSnapshot) -> Result<()> {
        if !matches_queue(desired, actual) {
            return Err(Error::State("FM queue update requires recovery"));
        }
        Ok(())
    }
    pub(super) async fn enter(
        &self,
        token: &SessionToken,
        source: FmSource,
        quality: Quality,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        self.guard(token, intent)?;
        source.validate()?;
        let (mut before, actual) = self.controller.queue_with_state(token).await?;
        if actual
            .intelligence
            .as_ref()
            .is_some_and(crate::playback::intelligence::IntelligenceContext::active)
        {
            return Err(Error::State("leave intelligence before entering FM"));
        }
        let active = self::source(&actual)?;
        if active.as_ref() == Some(&source) {
            let desired = self
                .service
                .queue_for(token, &source)?
                .ok_or(Error::State("FM cache is missing"))?;
            if matches_queue(&desired, &actual)
                && matches!(
                    before.state,
                    PlaybackStatus::Playing | PlaybackStatus::Buffering | PlaybackStatus::Paused
                )
            {
                self.guard(token, intent)?;
                return self
                    .controller
                    .command(
                        token,
                        PlaybackCommand::Pause(before.state != PlaybackStatus::Paused),
                    )
                    .await;
            }
        }
        self.service.refresh_blacklist_for(token).await?;
        self.service.refresh_trash_for(token).await?;
        let mut desired = match self.service.queue_for(token, &source)? {
            Some(q) if q.current().is_some() => q,
            _ => {
                let rights = crate::membership::MembershipManager::new(self.session.clone())?
                    .refresh_for(token.clone())
                    .await?;
                self.service
                    .preload_for(token, source, FmRights::from(&rights))
                    .await?
            }
        };
        // Official fmPlay resets to infinite. Resume/restore below preserve an
        // already selected single-repeat mode instead of acting as fmPlay.
        desired = self
            .service
            .playable_for(token, &desired.source, desired.revision)
            .await?;
        desired = self.service.repeat_for(
            token,
            &desired.source,
            desired.revision,
            FmRepeat::Infinite,
        )?;
        self.guard(token, intent)?;
        let has_backup = self.session.with_token(token, |uid| {
            Ok(self.store.get(uid, "playback", FM_BACKUP_QUEUE)?.is_some())
        })?;
        if active.is_none() && (!actual.items.is_empty() || !has_backup) {
            self.backup(token, &before, &actual)?;
        }
        self.session.with_token(token, |uid| {
            self.store.put(
                uid,
                "fm-player-v1",
                "source",
                &serde_json::to_value(&desired.source)?,
            )
        })?;
        before.requested_quality = Some(quality.as_str().into());
        self.apply(
            token,
            &before,
            &desired,
            intent,
            Some(QueueTransport {
                position: Some(0.0),
                paused: false,
            }),
        )
        .await
    }
    pub(super) async fn leave(
        &self,
        token: &SessionToken,
        intent: u64,
    ) -> Result<PlaybackSnapshot> {
        let _gate = self.gate.try_lock().map_err(|_| Error::MutationBusy)?;
        let view = self
            .controller
            .command(token, PlaybackCommand::Snapshot)
            .await?;
        self.guard(token, intent)?;
        let input = self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                Ok(crate::playback::controller::FmRestore {
                    expected: view.fence(),
                    backup_queue: tx.get(uid, "playback", FM_BACKUP_QUEUE)?,
                    backup_position: tx.get(uid, "playback", FM_BACKUP_POSITION)?,
                })
            })
        })?;
        let intents = self.intents.clone();
        let result = self
            .controller
            .command_guarded(
                token,
                PlaybackCommand::RestoreFm(input),
                view.fence(),
                crate::playback::engine::Cancellation::new(move || {
                    intents.load(Ordering::Acquire) == intent
                }),
            )
            .await?;
        self.guard(token, intent)?;
        Ok(result)
    }
}
