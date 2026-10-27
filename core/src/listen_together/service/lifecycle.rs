use super::*;

/// M/75 END_LISTEN_TOGETHER: durable before any status/end request. Its
/// presence disables playback reports until the room is confirmed gone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndNotice {
    pub room_id: String,
    pub received_at_ms: u64,
}
impl TogetherService {
    pub fn receive_end_for(&self, token: &SessionToken, room: &str, now: u64) -> Result<bool> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        if state.role == Some(RoomRole::Waiting) || state.end_notice.is_some() {
            return Ok(false);
        }
        state.end_notice = Some(EndNotice {
            room_id: room.into(),
            received_at_ms: now,
        });
        self.session
            .with_token(token, |owner| self.save(owner, &state, None))?;
        Ok(true)
    }

    /// Invoke once on receipt, or explicitly for recovery. A previously sent
    /// end is only reconciled; a duplicate notification cannot resend it.
    pub async fn resolve_end_notice_for(
        &self,
        token: &SessionToken,
        room: &str,
        now: u64,
    ) -> Result<()> {
        let state = self.state_for(token)?;
        if state
            .end_notice
            .as_ref()
            .is_none_or(|notice| notice.room_id != room)
        {
            return Err(Error::State("no matching together end notice"));
        }
        let prior = self
            .operations_for(token)?
            .into_iter()
            .filter(|op| {
                op.phase.pending()
                    && matches!(&op.intent, WriteIntent::End { room_id, .. } if room_id == room)
            })
            .collect::<Vec<_>>();
        for op in prior {
            if self
                .reconcile_for(token, &op.id, now)
                .await?
                .phase
                .pending()
            {
                return Err(Error::MutationUncertain);
            }
        }
        let state = self.refresh_for(token, now).await?;
        if state.room.as_ref().is_some_and(|r| r.room_id == room)
            && self.end_for(token, room, None, now).await?.phase != TogetherPhase::Confirmed
        {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
}
