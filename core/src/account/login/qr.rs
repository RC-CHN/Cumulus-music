//! QR authorization, identity read and profile commit as one candidate flow.
use super::{
    LoginStatus,
    state::{Operation, State},
};
use crate::{
    Error, Result,
    account::{
        AccountSession, QrLogin, QrSnapshot, QrStatus, SessionToken, profiles::LoginCandidate,
        session::next_generation,
    },
};
use std::sync::{Arc, Mutex};

pub struct QrAuthentication {
    session: Arc<AccountSession>,
    candidate: LoginCandidate,
    base: SessionToken,
    qr: QrLogin,
    state: Mutex<State>,
}

impl QrAuthentication {
    pub fn new(session: Arc<AccountSession>, candidate: LoginCandidate) -> Result<Self> {
        let base = session.token()?;
        let qr = QrLogin::new(candidate.api());
        Ok(Self {
            session,
            candidate,
            base,
            qr,
            state: Mutex::new(State::default()),
        })
    }
    pub fn snapshot(&self) -> Result<QrSnapshot> {
        self.qr.snapshot()
    }
    pub fn authenticated(&self) -> Result<bool> {
        Ok(
            self.state.lock().map_err(|_| Error::LockPoisoned)?.status
                == LoginStatus::Authenticated,
        )
    }
    pub async fn begin(&self) -> Result<String> {
        let operation = Operation::begin(&self.state, LoginStatus::Submitting, None)?;
        let response = self.qr.begin().await;
        operation.lock()?.status = LoginStatus::Ready;
        operation.finish(response)
    }
    pub fn cancel(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.status == LoginStatus::Authenticated {
            return Err(Error::State(
                "completed login cannot be cancelled; use logout",
            ));
        }
        state.generation = next_generation(state.generation)?;
        state.status = LoginStatus::Cancelled;
        state.busy = false;
        self.qr.cancel()
    }
    async fn identity(&self, operation: &Operation<'_>) -> Result<()> {
        operation.check()?;
        let response = self.candidate.api().account().await;
        let mut state = operation.lock()?;
        self.candidate.adopt(&self.session, &self.base, response?)?;
        state.status = LoginStatus::Authenticated;
        Ok(())
    }
    pub async fn poll(&self, completed_captcha: Option<&str>) -> Result<QrStatus> {
        let operation = Operation::begin(&self.state, LoginStatus::CheckingIdentity, None)?;
        let result = async {
            let status = self.qr.poll(completed_captcha).await;
            operation.check()?;
            let status = status?;
            if status == QrStatus::Authorized {
                self.identity(&operation).await?;
            } else {
                operation.lock()?.status = LoginStatus::Ready;
            }
            Ok(status)
        }
        .await;
        operation.finish(result)
    }
    /// Recover a lost authorization response by identity read, never by replay.
    pub async fn reconcile(&self) -> Result<()> {
        let operation = Operation::begin(&self.state, LoginStatus::CheckingIdentity, None)?;
        let result = self.identity(&operation).await;
        operation.finish(result)
    }
}
