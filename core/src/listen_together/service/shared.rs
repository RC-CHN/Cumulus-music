use super::*;
use crate::listen_together::CommonPrivilege;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedReception {
    pub id: String,
    pub room_id: String,
    pub phase: IncomingPhase,
    pub ignore_play_command: bool,
    pub snapshot: Option<TogetherSnapshot>,
    pub privileges: Vec<CommonPrivilege>,
    pub received_at_ms: u64,
}
impl TogetherService {
    /// M/3079: equal list versions still refresh common access; changed versions
    /// read the authoritative queue. Persist the obligation before either read.
    pub async fn receive_shared_for(
        &self,
        token: &SessionToken,
        room: &str,
        version: Option<&[Version]>,
        ignore_play_command: bool,
        now: u64,
    ) -> Result<SharedReception> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.require_active(token)?;
        self.require_resolved(token)?;
        let mut state = self.state_for(token)?;
        let members = &self.require_room(&state, room)?.members;
        let others = members
            .iter()
            .filter(|m| m.user_id != owner)
            .collect::<Vec<_>>();
        if others.len() != 1 {
            return Err(Error::State("common playback requires exactly one peer"));
        }
        let peer = others[0].user_id.clone();
        let mut receipt = SharedReception {
            id: uuid::Uuid::new_v4().simple().to_string(),
            room_id: room.into(),
            phase: IncomingPhase::Pending,
            ignore_play_command,
            snapshot: None,
            privileges: vec![],
            received_at_ms: now,
        };
        state.shared = Some(receipt.clone());
        self.session
            .with_token(token, |uid| self.save(uid, &state, None))?;
        let result: Result<_> = async {
            let snapshot = if version.is_some_and(|v| {
                state
                    .snapshot
                    .playlist
                    .as_ref()
                    .is_some_and(|p| versions_equal(v, &p.version))
            }) {
                state.snapshot.clone()
            } else {
                self.snapshot(token, room).await?
            };
            let playlist = snapshot
                .playlist
                .as_ref()
                .ok_or(Error::Response("shared queue missing"))?;
            if playlist.mode == super::super::SharedMode::Random && playlist.random.is_none() {
                return Err(Error::Response("random shared order missing"));
            }
            let privileges = if playlist.display.is_empty() {
                vec![]
            } else {
                let rows = self
                    .session
                    .run_for(token, |api, _| async {
                        TogetherApi::new(api)
                            .privileges(&peer, &playlist.display)
                            .await
                    })
                    .await?;
                CommonPrivilege::from_rows(&playlist.display, &rows)?
            };
            Ok((snapshot, privileges))
        }
        .await;
        match result {
            Ok((snapshot, privileges)) => {
                if !ignore_play_command {
                    if let Some(command) = &snapshot.play_command {
                        state.playback.restore(command.clone(), &owner, now)?;
                    }
                    state.snapshot.play_command = snapshot.play_command.clone();
                }
                state.snapshot.playlist = snapshot.playlist.clone();
                receipt.snapshot = Some(snapshot);
                receipt.privileges = privileges;
                state.shared = Some(receipt.clone());
                self.session
                    .with_token(token, |uid| self.save(uid, &state, None))?;
                Ok(receipt)
            }
            Err(error) => {
                receipt.phase = IncomingPhase::Failed;
                state.shared = Some(receipt);
                self.session
                    .with_token(token, |uid| self.save(uid, &state, None))?;
                Err(error)
            }
        }
    }
    pub fn finish_shared_for(
        &self,
        token: &SessionToken,
        room: &str,
        id: &str,
        phase: IncomingPhase,
    ) -> Result<()> {
        if phase == IncomingPhase::Pending {
            return Err(Error::InvalidInput("shared completion cannot be pending"));
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        let shared = state
            .shared
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or(Error::StaleResponse)?;
        if matches!(
            shared.phase,
            IncomingPhase::Applied | IncomingPhase::Superseded
        ) {
            return if shared.phase == phase {
                Ok(())
            } else {
                Err(Error::StaleResponse)
            };
        }
        shared.phase = phase;
        if phase == IncomingPhase::Applied
            && !shared.ignore_play_command
            && let Some(sequence) = shared
                .snapshot
                .as_ref()
                .and_then(|s| s.play_command.as_ref())
                .and_then(|c| c.server_seq)
            && let Some(incoming) = &mut state.incoming
            && matches!(
                incoming.phase,
                IncomingPhase::Pending | IncomingPhase::Failed
            )
            && incoming.command.server_seq.is_some_and(|s| s <= sequence)
        {
            incoming.phase = IncomingPhase::Superseded;
        }
        self.session
            .with_token(token, |uid| self.save(uid, &state, None))
    }
}
