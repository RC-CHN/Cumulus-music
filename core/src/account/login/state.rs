use super::{LoginStatus, request::LoginRequest};
use crate::{Error, Result};
use serde_json::Value;
use std::sync::{Mutex, MutexGuard};

pub(super) struct State {
    pub generation: u64,
    pub status: LoginStatus,
    pub code: Option<i64>,
    pub busy: bool,
    pub error_kind: Option<&'static str>,
    pub pending: Option<LoginRequest>,
    pub challenge: Option<Value>,
    pub verification: Option<LoginStatus>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            generation: 0,
            status: LoginStatus::Ready,
            code: None,
            busy: false,
            error_kind: None,
            pending: None,
            challenge: None,
            verification: None,
        }
    }
}

pub(super) struct Operation<'a> {
    state: &'a Mutex<State>,
    generation: u64,
    finished: bool,
}

impl<'a> Operation<'a> {
    pub fn begin(
        state: &'a Mutex<State>,
        status: LoginStatus,
        expected: Option<u64>,
    ) -> Result<Self> {
        let mut current = state.lock().map_err(|_| Error::LockPoisoned)?;
        if expected.is_some_and(|g| g != current.generation) {
            return Err(Error::StaleResponse);
        }
        if matches!(
            current.status,
            LoginStatus::Cancelled | LoginStatus::Authenticated
        ) {
            return Err(Error::State("login attempt is terminal"));
        }
        if current.busy {
            return Err(Error::State("login operation is already pending"));
        }
        current.busy = true;
        current.status = status;
        current.error_kind = None;
        Ok(Self {
            state,
            generation: current.generation,
            finished: false,
        })
    }
    pub fn lock(&self) -> Result<MutexGuard<'_, State>> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != self.generation {
            return Err(Error::StaleResponse);
        }
        Ok(state)
    }
    pub fn check(&self) -> Result<()> {
        drop(self.lock()?);
        Ok(())
    }
    pub fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        {
            let mut state = self.lock()?;
            state.busy = false;
            if let Err(error) = &result {
                state.status = LoginStatus::Error;
                state.error_kind = Some(match error {
                    Error::Transport(_) => "transport",
                    Error::StaleResponse | Error::CredentialChanged => "superseded",
                    Error::Storage(_) | Error::Io(_) => "persistence",
                    Error::Service { .. } | Error::HttpStatus { .. } => "service",
                    Error::Response(_) | Error::Protocol(_) | Error::Json(_) => "response",
                    _ => "state",
                });
            }
        }
        self.finished = true;
        result
    }
}

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if !self.finished
            && let Ok(mut state) = self.state.lock()
            && state.generation == self.generation
        {
            state.busy = false;
            state.status = LoginStatus::Error;
            state.error_kind = Some("interrupted");
        }
    }
}
