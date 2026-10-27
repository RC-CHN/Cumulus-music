use super::*;
use crate::listen_together::WriteIntent;

pub(super) const NOTICE: &str = "listen-together-lifecycle-v1";

/// Shared by explicit exit and authoritative automatic exit. A finishing old
/// runtime may only undo its own room's binding and queue constraints.
pub(super) async fn restore_local(
    session: &Arc<AccountSession>,
    store: Arc<StateStore>,
    controller: &PlaybackController,
    token: &SessionToken,
    expected_room: Option<&str>,
) -> Result<bool> {
    let saved: Option<Binding> = session.with_token(token, |uid| {
        store
            .get(uid, BINDING, "current")?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    })?;
    let Some(saved) = saved else {
        return Ok(false);
    };
    if expected_room.is_some_and(|room| saved.room_id != room) {
        return Err(Error::StaleResponse);
    }
    reconcile_closed_binding(session, token, &saved.room_id).await?;
    let (mut current, mut queue) = controller.queue_with_state(token).await?;
    let mut changed = false;
    for item in &mut queue.items {
        if item.resource["shared_room"].as_str() == Some(&saved.room_id) {
            let object = item
                .resource
                .as_object_mut()
                .ok_or(Error::State("invalid shared resource"))?;
            object.remove("playback_constraint");
            object.remove("shared_room");
            changed = true;
        }
    }
    if changed {
        current = controller
            .command(
                token,
                PlaybackCommand::UpdateQueue(crate::playback::controller::QueueUpdate {
                    expected: current.fence(),
                    queue,
                    transport: None,
                }),
            )
            .await?;
    }
    if current.speed == 1.0 {
        controller
            .command(token, PlaybackCommand::Speed(saved.previous_speed))
            .await?;
    }
    if current.mode == PlaybackMode::OrderLoop && saved.previous_mode == PlaybackMode::Sequential {
        controller
            .command(token, PlaybackCommand::Mode(PlaybackMode::Sequential))
            .await?;
    }
    session.with_token(token, |uid| store.delete(uid, BINDING, "current"))?;
    Ok(true)
}

async fn reconcile_closed_binding(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    room: &str,
) -> Result<()> {
    // The caller owns the host gate, while each service action owns the
    // cross-process operation lock. Only read reconciliation is permitted.
    let service = TogetherService::new(session.clone())?;
    for op in service.operations_for(token)? {
        let bound = match &op.intent {
            WriteIntent::Create { .. } => {
                op.created_room.as_ref().is_some_and(|r| r.room_id == room)
            }
            WriteIntent::Accept { room_id, .. }
            | WriteIntent::End { room_id, .. }
            | WriteIntent::Queue { room_id, .. }
            | WriteIntent::Play { room_id, .. } => room_id == room,
        };
        if bound
            && op.phase.pending()
            && service
                .reconcile_for(token, &op.id, now())
                .await?
                .phase
                .pending()
        {
            return Err(Error::MutationUncertain);
        }
    }
    if service
        .state_for(token)?
        .room
        .as_ref()
        .is_some_and(|r| r.room_id == room)
    {
        return Err(Error::State("together room returned during cleanup"));
    }
    Ok(())
}

impl runtime::Context {
    pub(super) async fn close_im(&self) -> Result<()> {
        if let Some(im) = &self.im
            && let Err(error) = im.close().await
        {
            *self
                .cleanup_uncertain
                .lock()
                .map_err(|_| Error::LockPoisoned)? = true;
            return Err(error);
        }
        Ok(())
    }
    /// The REST worker is already leaving its loop. Close its SDK connection,
    /// settle old writes by reads, and restore local state without ending any
    /// different room that has since appeared on the account.
    pub(super) async fn finish_runtime(&self) -> Result<()> {
        let _gate = self.gate.lock().await;
        self.close_im().await?;
        self.session.with_token(&self.token, |_| Ok(()))?;
        let service = TogetherService::new(self.session.clone())?;
        if service
            .state_for(&self.token)?
            .room
            .as_ref()
            .is_some_and(|r| r.room_id == self.room_id)
        {
            return Ok(());
        }
        // Another process/service can publish room=None during its first
        // confirming read while it still owns the end-operation lock. Wait
        // for that owner to settle; cleanup uses read reconciliation only and
        // never resends an end request. All other errors remain explicit.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let restored = loop {
            match restore_local(
                &self.session,
                self.store.clone(),
                &self.controller,
                &self.token,
                Some(&self.room_id),
            )
            .await
            {
                Err(Error::MutationBusy) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                result => break result?,
            }
        };
        if restored {
            self.session.with_token(&self.token, |uid| {
                self.store.put(
                    uid,
                    NOTICE,
                    "current",
                    &json!("一起听已结束，已恢复本机播放设置"),
                )
            })?;
            *self.error.lock().map_err(|_| Error::LockPoisoned)? = None;
        }
        Ok(())
    }
}
