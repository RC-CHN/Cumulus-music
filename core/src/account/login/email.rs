//! Candidate-owned security email verification. PC selects -> send, six-digit
//! PIN -> verify; only an acknowledged step permits identity reconciliation.
use super::{CredentialLogin, LoginStatus, request::LoginRequest, state::Operation};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailPhase {
    Sending,
    CodeRequired,
    Verifying,
    SendLimit,
    VerifyLimit,
    Failed,
    SendUnconfirmed,
    RecoveryRequired,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailVerificationView {
    pub id: String,
    pub phase: EmailPhase,
    pub code: Option<i64>,
    pub mail_address: Option<String>,
    pub answer_rejected: bool,
}
pub(super) struct Attempt {
    pub view: EmailVerificationView,
    pub cancel: tokio::sync::watch::Sender<bool>,
}
impl CredentialLogin {
    pub fn email_snapshot(&self) -> Result<Option<EmailVerificationView>> {
        Ok(self.lock()?.email.as_ref().map(|a| a.view.clone()))
    }
    /// There is no recovered remote email-cancel endpoint. Revoke only the
    /// local attempt; an email already accepted by the service may arrive.
    pub fn email_stop(&self, id: &str) -> Result<()> {
        let mut state = self.lock()?;
        let item = state
            .email
            .as_ref()
            .filter(|a| a.view.id == id)
            .ok_or(Error::StaleResponse)?;
        item.cancel.send_replace(true);
        state.generation = crate::account::session::next_generation(state.generation)?;
        state.email = None;
        state.status = LoginStatus::TwoFactor;
        state.busy = false;
        Ok(())
    }
    pub async fn email_start(&self) -> Result<()> {
        self.email_send(None).await
    }
    pub async fn email_resend(&self, id: &str) -> Result<()> {
        self.email_send(Some(id)).await
    }
    async fn email_send(&self, previous: Option<&str>) -> Result<()> {
        let generation = {
            let state = self.lock()?;
            ready_to_send(&state, previous)?;
            state.generation
        };
        let operation = Operation::begin_checked(
            &self.state,
            LoginStatus::Verifying,
            Some(generation),
            |state| ready_to_send(state, previous),
        )?;
        let (cancel, _) = tokio::sync::watch::channel(false);
        operation.lock()?.email = Some(Attempt {
            cancel: cancel.clone(),
            view: EmailVerificationView {
                id: uuid::Uuid::new_v4().to_string(),
                phase: EmailPhase::Sending,
                code: None,
                mail_address: None,
                answer_rejected: false,
            },
        });
        let mut stopped = cancel.subscribe();
        let work = async {
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/user/2fa/securityEmail/send",
                        payload: json!({"scene":"0"}),
                    },
                    false,
                    Some(LoginStatus::TwoFactor),
                )
                .await?;
            let mut state = operation.lock()?;
            let item = state.email.as_mut().ok_or(Error::StaleResponse)?;
            item.view.code = response["code"].as_i64();
            item.view.phase = if response["code"] == 4011 {
                EmailPhase::SendLimit
            } else if accepted(&response) {
                item.view.mail_address = response["data"]["mailAddress"]
                    .as_str()
                    .filter(|s| {
                        !s.is_empty()
                            && s.chars().count() <= 256
                            && s.bytes().filter(|c| *c == b'@').count() == 1
                            && !s.starts_with('@')
                            && !s.ends_with('@')
                            && !s.chars().any(|c| {
                                c.is_control() || c.is_whitespace() || ":/\\?#".contains(c)
                            })
                    })
                    .map(str::to_owned);
                EmailPhase::CodeRequired
            } else {
                EmailPhase::Failed
            };
            state.status = LoginStatus::TwoFactor;
            Ok(())
        };
        let result = tokio::select! {
            biased;
            _ = stopped.changed() => Err(Error::StaleResponse),
            result = tokio::time::timeout(std::time::Duration::from_secs(45), work) => result.unwrap_or_else(|_|Err(Error::Transport("email send timeout".into()))),
        };
        if let Err(error) = &result
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.email
        {
            item.view.phase = if matches!(error.root(), Error::RequestNotSent(_)) {
                EmailPhase::Failed
            } else {
                EmailPhase::SendUnconfirmed
            };
        }
        operation.finish(result)
    }
    pub async fn email_verify(&self, id: &str, code: &str) -> Result<bool> {
        if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidInput(
                "email verification requires six ASCII digits",
            ));
        }
        let (generation, cancel, previous_phase) = {
            let state = self.lock()?;
            let item = state
                .email
                .as_ref()
                .filter(|a| a.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if !state.email_allowed
                || !matches!(
                    item.view.phase,
                    EmailPhase::CodeRequired | EmailPhase::SendUnconfirmed
                )
            {
                return Err(Error::State("email is not ready for verification"));
            }
            // Keep the sender alive through identity/replay: successful adoption
            // or a new challenge removes the attempt from State.
            (state.generation, item.cancel.clone(), item.view.phase)
        };
        let operation = Operation::begin_checked(
            &self.state,
            LoginStatus::Verifying,
            Some(generation),
            |state| {
                let item = state
                    .email
                    .as_ref()
                    .filter(|a| a.view.id == id)
                    .ok_or(Error::StaleResponse)?;
                if !state.email_allowed
                    || !matches!(
                        item.view.phase,
                        EmailPhase::CodeRequired | EmailPhase::SendUnconfirmed
                    )
                {
                    return Err(Error::State("email is not ready for verification"));
                }
                Ok(())
            },
        )?;
        {
            let mut state = operation.lock()?;
            let item = state.email.as_mut().ok_or(Error::StaleResponse)?;
            item.view.phase = EmailPhase::Verifying;
            item.view.answer_rejected = false;
            item.view.code = None;
        }
        let mut stopped = cancel.subscribe();
        let work = async {
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/user/2fa/securityEmail/verify",
                        payload: json!({"scene":"0","captcha":code}),
                    },
                    false,
                    Some(LoginStatus::TwoFactor),
                )
                .await?;
            let success = accepted(&response);
            {
                let mut state = operation.lock()?;
                if matches!(response["code"].as_i64(), Some(8830 | 8860 | 8831)) {
                    state.email = None;
                    state.email_allowed = false;
                    return Ok(true); // Discard the previous method-list projection.
                }
                let item = state.email.as_mut().ok_or(Error::StaleResponse)?;
                item.view.code = response["code"].as_i64();
                item.view.answer_rejected = !success;
                item.view.phase = match response["code"].as_i64() {
                    Some(4011) => EmailPhase::SendLimit,
                    Some(4013) => EmailPhase::VerifyLimit,
                    _ if success => EmailPhase::RecoveryRequired,
                    _ => EmailPhase::CodeRequired,
                };
                state.status = LoginStatus::TwoFactor;
            }
            if success {
                self.complete_verification(&operation).await?;
            }
            Ok(success)
        };
        let result = tokio::select! {
            biased;
            _ = stopped.changed() => Err(Error::StaleResponse),
            result = tokio::time::timeout(std::time::Duration::from_secs(45), work) => result.unwrap_or_else(|_|Err(Error::Transport("email verification timeout".into()))),
        };
        if let Err(error) = &result
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.email
        {
            item.view.phase = if matches!(error.root(), Error::RequestNotSent(_)) {
                previous_phase
            } else {
                EmailPhase::RecoveryRequired
            };
        }
        operation.finish(result)
    }
}
fn accepted(value: &serde_json::Value) -> bool {
    value["code"] == 200 && value["data"] != false && value["data"]["success"] != false
}

fn ready_to_send(state: &super::state::State, previous: Option<&str>) -> Result<()> {
    if !state.email_allowed
        || state.pending.is_none()
        || state.verification != Some(LoginStatus::TwoFactor)
        || state.sms.is_some()
        || state.device.as_ref().is_some_and(|a| {
            matches!(
                a.view.phase,
                super::DeviceVerificationPhase::Starting
                    | super::DeviceVerificationPhase::Waiting
                    | super::DeviceVerificationPhase::Scanned
                    | super::DeviceVerificationPhase::CodeRequired
                    | super::DeviceVerificationPhase::Error
                    | super::DeviceVerificationPhase::RecoveryRequired
            )
        })
    {
        return Err(Error::State(
            "security email was not offered or another method is active",
        ));
    }
    match (&state.email, previous) {
        (None, None) => {}
        (Some(item), Some(id))
            if item.view.id == id
                && matches!(
                    item.view.phase,
                    EmailPhase::VerifyLimit | EmailPhase::Failed
                ) => {}
        (Some(item), Some(id)) if item.view.id != id => return Err(Error::StaleResponse),
        _ => return Err(Error::State("email cannot be sent again in this state")),
    }
    Ok(())
}
