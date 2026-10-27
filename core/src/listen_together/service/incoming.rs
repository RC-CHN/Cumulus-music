use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncomingPhase {
    Pending,
    Applied,
    Superseded,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IncomingPlayback {
    pub command: PlayCommand,
    pub received_at_ms: u64,
    pub phase: IncomingPhase,
}
impl TogetherService {
    /// Persist official server-sequence/echo reduction before native playback.
    /// A crash/cancel leaves a visible pending application, never an applied lie.
    pub fn receive_play_for(
        &self,
        token: &SessionToken,
        room: &str,
        command: PlayCommand,
        current: Option<&str>,
        playing: bool,
        now: u64,
    ) -> Result<Option<IncomingPlayback>> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.require_active(token)?;
        self.require_resolved(token)?;
        self.require_shared_applied(token)?;
        let mut state = self.state_for(token)?;
        let room = self.require_room(&state, room)?;
        if room.role_for(&owner)? == RoomRole::Waiting {
            return Ok(None);
        }
        if command
            .user_id
            .as_ref()
            .is_some_and(|id| !room.members.iter().any(|m| &m.user_id == id))
        {
            return Err(Error::Response(
                "together command author is not a room member",
            ));
        }
        let allowed = state
            .snapshot
            .playlist
            .as_ref()
            .ok_or(Error::State("shared queue not loaded"))?
            .display
            .clone();
        if !state
            .playback
            .incoming(command.clone(), &allowed, current, playing, now)?
        {
            return Ok(None);
        }
        let incoming = IncomingPlayback {
            command: command.clone(),
            received_at_ms: now,
            phase: IncomingPhase::Pending,
        };
        state.snapshot.play_command = Some(command);
        state.incoming = Some(incoming.clone());
        self.session
            .with_token(token, |uid| self.save(uid, &state, None))?;
        Ok(Some(incoming))
    }
    pub fn finish_incoming_for(
        &self,
        token: &SessionToken,
        room: &str,
        server_seq: u64,
        phase: IncomingPhase,
    ) -> Result<()> {
        if phase == IncomingPhase::Pending {
            return Err(Error::InvalidInput("incoming completion cannot be pending"));
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        let incoming = state
            .incoming
            .as_mut()
            .filter(|v| v.command.server_seq == Some(server_seq))
            .ok_or(Error::StaleResponse)?;
        if matches!(
            incoming.phase,
            IncomingPhase::Applied | IncomingPhase::Superseded
        ) {
            return if incoming.phase == phase {
                Ok(())
            } else {
                Err(Error::StaleResponse)
            };
        }
        incoming.phase = phase;
        self.session
            .with_token(token, |uid| self.save(uid, &state, None))
    }
}
