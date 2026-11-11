use super::*;
use crate::listen_together::{TogetherOperation, WriteIntent};

/// One display receipt in this host and account epoch. It never goes into
/// durable state, including the journal used for unknown remote outcomes.
#[derive(Clone, Default)]
pub(super) struct LeaveRecords(Arc<Mutex<Option<ScopedRecord>>>);

struct ScopedRecord {
    token: SessionToken,
    record: TogetherLeaveRecordView,
}

impl LeaveRecords {
    pub(super) fn publish(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        expected_room: &str,
        operation: TogetherOperation,
    ) -> Result<()> {
        if !operation.acknowledged
            || operation.phase != TogetherPhase::Confirmed
            || operation.observed_room.is_some()
            || !matches!(operation.intent, WriteIntent::End { room_id, .. } if room_id == expected_room)
        {
            return Ok(());
        }
        let Some(record) = operation.leave_record else {
            return Ok(());
        };
        session.with_token(token, |_| {
            *self.0.lock().map_err(|_| Error::LockPoisoned)? = Some(ScopedRecord {
                token: token.clone(),
                record: TogetherLeaveRecordView {
                    operation_id: operation.id,
                    this_listen_count: record.this_listen_count,
                    this_duration: record.this_duration,
                    total_listen_count: record.total_listen_count,
                    total_duration: record.total_duration,
                    self_avatar_url: record.self_avatar_url,
                    peer_avatar_url: record.peer_avatar_url,
                },
            });
            Ok(())
        })
    }

    pub(super) fn dismiss(&self, token: &SessionToken, operation_id: &str) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot
            .as_ref()
            .is_none_or(|item| item.token != *token || item.record.operation_id != operation_id)
        {
            return Err(Error::StaleResponse);
        }
        slot.take();
        Ok(())
    }

    pub(super) fn retire_other_epoch(&self, token: &SessionToken) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot
            .as_ref()
            .is_some_and(|item| item.token.generation < token.generation)
        {
            slot.take();
        }
        Ok(())
    }

    pub(super) fn room_observed(&self, token: &SessionToken) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.as_ref().is_some_and(|item| item.token == *token) {
            slot.take();
        }
        Ok(())
    }

    pub(super) fn retire_obsolete(&self, token: &SessionToken, operation_id: &str) -> Result<()> {
        match self.dismiss(token, operation_id) {
            Err(Error::StaleResponse) => Ok(()),
            result => result,
        }
    }

    pub(super) fn current(&self, token: &SessionToken) -> Result<Option<TogetherLeaveRecordView>> {
        let slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(slot
            .as_ref()
            .filter(|item| item.token == *token)
            .map(|item| item.record.clone()))
    }
}
