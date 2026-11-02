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
    pub device: Option<super::device::Attempt>,
    pub sms_context: Option<super::sms::Context>,
    pub sms: Option<super::sms::Attempt>,
    pub email_allowed: bool,
    pub email: Option<super::email::Attempt>,
    pub question: Option<super::question::Attempt>,
    pub reuse: Option<super::reuse::Attempt>,
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
            device: None,
            sms_context: None,
            sms: None,
            email_allowed: false,
            email: None,
            question: None,
            reuse: None,
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
        Self::begin_checked(state, status, expected, |_| Ok(()))
    }
    pub fn begin_checked(
        state: &'a Mutex<State>,
        status: LoginStatus,
        expected: Option<u64>,
        ready: impl FnOnce(&State) -> Result<()>,
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
        ready(&current)?;
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
            if let Some(sms) = &mut state.sms {
                sms.view.phase = super::SmsPhase::RecoveryRequired;
            }
            if let Some(email) = &mut state.email {
                email.view.phase = if email.view.phase == super::EmailPhase::Sending {
                    super::EmailPhase::SendUnconfirmed
                } else {
                    super::EmailPhase::RecoveryRequired
                };
            }
            if let Some(question) = &mut state.question {
                question.view.phase = if question.view.phase == super::QuestionPhase::Loading {
                    super::QuestionPhase::Failed
                } else {
                    super::QuestionPhase::RecoveryRequired
                };
            }
            if let Some(reuse) = &mut state.reuse {
                use super::ReusePhase;
                reuse.view.phase = match reuse.view.phase {
                    ReusePhase::Loading => ReusePhase::ReadFailed,
                    ReusePhase::Hinting | ReusePhase::CheckingNickname => ReusePhase::Nickname,
                    _ => ReusePhase::RecoveryRequired,
                };
            }
        }
    }
}
