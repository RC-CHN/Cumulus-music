use super::*;
use crate::listen_together::SharedMode;
impl TogetherService {
    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        id: &str,
        now: u64,
    ) -> Result<TogetherOperation> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let op = self
            .operations_for(token)?
            .into_iter()
            .find(|o| o.id == id)
            .ok_or(Error::InvalidInput("unknown together operation"))?;
        if !op.phase.pending() {
            return Ok(op);
        }
        self.settle(token, op, now).await
    }
    pub(super) async fn settle(
        &self,
        token: &SessionToken,
        mut op: TogetherOperation,
        now: u64,
    ) -> Result<TogetherOperation> {
        let owner = self.owner(token)?;
        let mut consecutive = 0;
        let mut closed_reads = 0;
        for attempt in 0..4 {
            if attempt > 0 {
                tokio::time::sleep(self.settle_delay).await;
            }
            let mut state = self.state_for(token)?;
            let room = self.status(token).await?;
            Self::observe(&mut state, room, &owner, now)?;
            op.observed_room = state.room.as_ref().map(|r| r.room_id.clone());
            let expected = op
                .intent
                .room_id()
                .or_else(|| op.created_room.as_ref().map(|r| r.room_id.as_str()));
            let same =
                expected.is_some() && state.room.as_ref().map(|r| r.room_id.as_str()) == expected;
            // A pending report must not permanently block future rooms after
            // its room expires or another participant ends it. This settles
            // responsibility, not whether the original write ever succeeded.
            let room_closed = !matches!(op.intent, WriteIntent::End { .. })
                && if expected.is_some() {
                    !same
                } else {
                    state.room.is_none()
                };
            closed_reads = if room_closed { closed_reads + 1 } else { 0 };
            let matches = match &op.intent {
                WriteIntent::Create { .. } => {
                    same && op
                        .created_room
                        .as_ref()
                        .is_some_and(|r| r.creator_id == owner)
                }
                WriteIntent::Accept { .. } => same,
                WriteIntent::End { room_id, .. } => {
                    state.room.as_ref().is_none_or(|r| r.room_id != *room_id)
                }
                WriteIntent::Queue {
                    command,
                    expected_display,
                    ..
                } if same => {
                    state.snapshot = self.snapshot(token, expected.unwrap()).await?;
                    state.snapshot.playlist.as_ref().is_some_and(|p| {
                        p.mode == command.play_mode
                            && versions_equal(&p.version, &command.version)
                            && if command.command_type == ListCommandKind::Add {
                                same_members(&p.display, expected_display)
                            } else {
                                p.display == *expected_display
                            }
                            && (command.play_mode != SharedMode::Random
                                || p.random.as_ref().is_some_and(|r| {
                                    if matches!(
                                        command.command_type,
                                        ListCommandKind::Replace | ListCommandKind::PlaymodeChange
                                    ) {
                                        r == &command.random_list
                                    } else {
                                        same_members(r, &p.display)
                                    }
                                }))
                    })
                }
                WriteIntent::Play { command, .. } if same => {
                    state.snapshot = self.snapshot(token, expected.unwrap()).await?;
                    state.snapshot.play_command.as_ref().is_some_and(|c| {
                        c.same_report(command) && c.user_id.as_deref() == Some(&owner)
                    })
                }
                _ => false,
            };
            consecutive = if matches { consecutive + 1 } else { 0 };
            op.phase = if consecutive >= 2 {
                TogetherPhase::Confirmed
            } else if closed_reads >= 2 {
                TogetherPhase::RoomClosed
            } else {
                TogetherPhase::Uncertain
            };
            self.session
                .with_token(token, |owner| self.save(owner, &state, Some(&op)))?;
            if !op.phase.pending() {
                if matches!(op.intent, WriteIntent::End { .. }) {
                    self.close_pending(token, expected, state.room.is_none())?;
                }
                return Ok(op);
            }
        }
        Ok(op)
    }
    fn close_pending(
        &self,
        token: &SessionToken,
        room: Option<&str>,
        account_alone: bool,
    ) -> Result<()> {
        let operations = self.operations_for(token)?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                for mut op in operations {
                    if op.phase.pending()
                        && (op.intent.room_id() == room
                            || (account_alone && matches!(op.intent, WriteIntent::Create { .. })))
                    {
                        op.phase = TogetherPhase::RoomClosed;
                        tx.put(owner, JOURNAL, &op.id, &serde_json::to_value(&op)?)?;
                    }
                }
                Ok(())
            })
        })
    }
}

fn same_members(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|id| b.contains(id))
}
