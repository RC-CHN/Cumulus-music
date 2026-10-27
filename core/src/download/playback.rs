//! Cooperative lease for a completed file held by a native decoder.
//! File operations publish their durable intent before waiting for this lease.
use super::{directory::DownloadDirectory, service::DownloadTask};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use std::sync::Arc;

pub(crate) fn lock(store: &StateStore, owner: &str, task: &str) -> Result<OperationLock> {
    OperationLock::acquire(
        store.directory(),
        "download-playback",
        &serde_json::to_string(&(owner, task))?,
    )
}

pub(crate) enum SourceState {
    Ready(Box<DownloadTask>),
    Changing,
    Missing,
}

pub(crate) fn state(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    task: &str,
) -> Result<SourceState> {
    session.with_token(token, |_| Ok(()))?;
    if DownloadDirectory::new(session.clone())?.active()?.is_some() {
        return Ok(SourceState::Changing);
    }
    session.with_token(token, |owner| {
        let Some(value) = session
            .persistent_store()?
            .get(owner, super::service::TASKS, task)?
        else {
            return Ok(SourceState::Missing);
        };
        let value: DownloadTask = serde_json::from_value(value)?;
        if value.id != task {
            return Err(Error::State("download playback identity mismatch"));
        }
        Ok(if value.file_operation.is_some() {
            SourceState::Changing
        } else {
            SourceState::Ready(Box::new(value))
        })
    })
}

pub(crate) fn acquire(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    task: &DownloadTask,
) -> Result<OperationLock> {
    let lease = session.with_token(token, |owner| {
        lock(session.persistent_store()?.as_ref(), owner, &task.id)
    })?;
    match state(session, token, &task.id)? {
        SourceState::Ready(current)
            if current.target == task.target
                && current.output_integrity == task.output_integrity
                && current.status == task.status =>
        {
            Ok(lease)
        }
        _ => Err(Error::State("download changed before video opened")),
    }
}
