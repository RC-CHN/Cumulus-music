use super::*;
impl TogetherService {
    /// Official M/75 onChatRoomMsg resets the idle heartbeat before dispatch.
    /// Native host must fence the music room/SDK chatroom and account generation.
    pub fn message_received_for(&self, token: &SessionToken, room: &str, now: u64) -> Result<()> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        state.next_heartbeat_ms = Some(now.saturating_add(20_000));
        self.session
            .with_token(token, |owner| self.save(owner, &state, None))
    }
    /// Host timer entry: 20s idle heartbeat and 10min owner waiting timeout.
    /// Returns false when no network action is due. A transport failure keeps
    /// the room for retry; explicit service rejection checks status and exits
    /// only the same room, matching M/75's failed-heartbeat leave behavior.
    pub async fn tick_for(&self, token: &SessionToken, now: u64) -> Result<bool> {
        let state = self.state_for(token)?;
        if state.end_notice.is_some() {
            return Ok(false);
        }
        if let Some(room) = &state.room {
            if state.waiting_until_ms.is_some_and(|t| now >= t) {
                // Timeout only applies to the still-single-member room. A peer
                // may have joined while the IM adapter was disconnected.
                let refreshed = self.refresh_for(token, now).await?;
                if refreshed.room.as_ref().map(|r| &r.room_id) == Some(&room.room_id)
                    && refreshed.role == Some(RoomRole::Waiting)
                {
                    self.end_for(token, &room.room_id, None, now).await?;
                    return Ok(true);
                }
                return Ok(false);
            }
            if state.next_heartbeat_ms.is_some_and(|t| now >= t) {
                let owner = self.owner(token)?;
                let _lock = self.lock(&owner)?;
                let mut state = self.state_for(token)?;
                self.require_room(&state, &room.room_id)?;
                state.next_heartbeat_ms = Some(now.saturating_add(20_000));
                self.session
                    .with_token(token, |owner| self.save(owner, &state, None))?;
                if let Some(command) = state
                    .playback
                    .latest
                    .as_ref()
                    .map(|v| &v.command)
                    .or(state.snapshot.play_command.as_ref())
                {
                    let result = self
                        .session
                        .run_for(token, |api, _| async move {
                            TogetherApi::new(api)
                                .heartbeat(&room.room_id, command)
                                .await
                        })
                        .await;
                    drop(_lock);
                    if let Err(error) = result {
                        if matches!(error.root(), Error::Service { .. }) {
                            let refreshed = self.refresh_for(token, now).await?;
                            if refreshed.room.as_ref().map(|r| &r.room_id) == Some(&room.room_id) {
                                self.end_for(token, &room.room_id, None, now).await?;
                            }
                        }
                        return Err(error);
                    }
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
