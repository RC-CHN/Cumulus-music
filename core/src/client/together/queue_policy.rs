//! A clear confirmation belongs to both the displayed queue and its room.
use super::*;
use crate::client::player::QueueClearContext;
use crate::listen_together::{
    IncomingPhase, RoomRole, TogetherService, transport::ConnectionState,
};

impl MusicClient {
    pub(in crate::client) fn queue_clear_context(
        &self,
        token: &SessionToken,
    ) -> Result<QueueClearContext> {
        if token.uid.is_none() {
            self.session.check_generation(token)?;
            return Ok(QueueClearContext {
                room_id: None,
                issue: None,
            });
        }
        let service = TogetherService::new(self.session.clone())?;
        let state = service.state_for(token)?;
        let room_id = state.room.as_ref().map(|r| r.room_id.clone());
        let issue = if room_id.is_none() {
            None
        } else if !self.together_attached(token, room_id.as_deref())? {
            Some("当前一起听房间尚未连接，请先查看房间状态")
        } else if state.end_notice.is_some()
            || service
                .operations_for(token)?
                .iter()
                .any(|op| op.phase.pending())
            || self
                .together
                .error
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .is_some()
            || state
                .shared
                .as_ref()
                .is_some_and(|v| v.phase != IncomingPhase::Applied)
            || state
                .incoming
                .as_ref()
                .is_some_and(|v| matches!(v.phase, IncomingPhase::Pending | IncomingPhase::Failed))
        {
            Some("一起听尚有未完成的同步，请先核对房间状态")
        } else if state.role != Some(RoomRole::Waiting)
            && self.together_connection(token)?.state != ConnectionState::Connected
        {
            Some("一起听实时连接尚未就绪，请先查看房间状态")
        } else {
            None
        };
        self.session.check_generation(token)?;
        Ok(QueueClearContext { room_id, issue })
    }

    /// Called while the host room gate is held, before cancelling any audio.
    pub(in crate::client) fn validate_queue_clear_room(
        &self,
        token: &SessionToken,
        expected_room: Option<&str>,
    ) -> Result<()> {
        let context = self.queue_clear_context(token)?;
        if context.room_id.as_deref() != expected_room {
            return Err(Error::QueueChanged);
        }
        if context.issue.is_some() {
            return Err(Error::State(
                "together queue requires recovery before clearing",
            ));
        }
        Ok(())
    }
}
