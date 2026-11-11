use super::*;

/// M/75 END_LISTEN_TOGETHER: durable before any status/end request. Its
/// presence disables playback reports until the room is confirmed gone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndNotice {
    pub room_id: String,
    pub received_at_ms: u64,
    #[serde(default)]
    pub queue_loss: Option<QueueRoomLoss>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueRoomLoss {
    pub report_id: String,
    pub end_sent: bool,
}

/// M/3079 uses the response JSON code (not a generic HTTP or auth error).
pub(super) fn queue_report_room_loss(error: &Error) -> bool {
    let Error::Service {
        endpoint,
        code,
        body,
    } = error.root()
    else {
        return false;
    };
    endpoint == crate::listen_together::api::LIST_COMMAND_REPORT
        && code.as_i64() == Some(488)
        && body["data"]["result"].as_bool() != Some(true)
}

impl TogetherService {
    pub(crate) fn check_queue_room_loss_for(
        &self,
        token: &SessionToken,
        room: &str,
        report_id: &str,
    ) -> Result<()> {
        let state = self.state_for(token)?;
        self.require_room(&state, room)?;
        let loss = state
            .end_notice
            .as_ref()
            .filter(|notice| notice.room_id == room)
            .and_then(|notice| notice.queue_loss.as_ref())
            .filter(|loss| loss.report_id == report_id)
            .ok_or(Error::State("together room loss confirmation changed"))?;
        if loss.end_sent {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }

    /// After host IM cleanup. Check the exact receipt again under the journal
    /// lock; observe by status only because the lost room's playlist may fail.
    pub(crate) async fn resolve_queue_room_loss_for(
        &self,
        token: &SessionToken,
        room: &str,
        report_id: &str,
        now: u64,
    ) -> Result<Option<TogetherOperation>> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        self.check_queue_room_loss_for(token, room, report_id)?;
        if self
            .operations_for(token)?
            .iter()
            .any(|op| op.phase.pending() && matches!(&op.intent, WriteIntent::End { .. }))
        {
            return Err(Error::MutationUncertain);
        }
        let mut state = self.state_for(token)?;
        let found = self.status(token).await?;
        Self::observe(&mut state, found, &owner, now)?;
        if self.require_room(&state, room).is_err() {
            self.session
                .with_token(token, |owner| self.save(owner, &state, None))?;
            return Ok(None);
        }
        state
            .end_notice
            .as_mut()
            .and_then(|notice| notice.queue_loss.as_mut())
            .ok_or(Error::State("together room loss confirmation changed"))?
            .end_sent = true;
        // The latch and Sending journal are committed together before the
        // request. A second host or lost reply cannot emit another End.
        let operation = self
            .send_with_state(
                token,
                WriteIntent::End {
                    room_id: room.into(),
                    count: Some(1),
                    queue_loss_report: Some(report_id.into()),
                },
                state,
                now,
            )
            .await?;
        if operation.phase != TogetherPhase::Confirmed {
            return Err(Error::MutationUncertain);
        }
        Ok(Some(operation))
    }

    pub fn receive_end_for(&self, token: &SessionToken, room: &str, now: u64) -> Result<bool> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let mut state = self.state_for(token)?;
        self.require_room(&state, room)?;
        if state.role == Some(RoomRole::Waiting)
            || state
                .end_notice
                .as_ref()
                .is_some_and(|notice| notice.queue_loss.as_ref().is_none_or(|loss| loss.end_sent))
        {
            return Ok(false);
        }
        state.end_notice = Some(EndNotice {
            room_id: room.into(),
            received_at_ms: now,
            queue_loss: None,
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
            .is_none_or(|notice| notice.room_id != room || notice.queue_loss.is_some())
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
            && self.leave_for(token, room, now).await?.phase != TogetherPhase::Confirmed
        {
            return Err(Error::MutationUncertain);
        }
        Ok(())
    }
}
