//! Official Phone -> Captchav2 -> optional Nicknamev2, with immutable native SMS binding.
mod model;
mod registration;
use super::{
    CredentialLogin, LoginStatus,
    request::{self, LoginRequest},
    state::Operation,
};
use crate::{Error, Result};
pub use model::{PhoneEntryIssue, PhoneEntryPhase, PhoneEntryView};
use serde_json::json;
use tokio::sync::watch;

pub(super) struct Attempt {
    pub view: PhoneEntryView,
    pub cancel: watch::Sender<bool>,
    phone: String,
    country: String,
    middle: bool,
    verified_sms: Option<String>,
    registration_started: bool,
    login_started: bool,
    send_started: bool,
}
impl Attempt {
    pub(super) fn interrupted(&mut self) {
        if self.registration_started || self.login_started {
            self.view.phase = PhoneEntryPhase::RecoveryRequired;
            self.view.issue = Some(PhoneEntryIssue::LoginUnconfirmed);
        } else if self.verified_sms.is_some() {
            self.view.phase = PhoneEntryPhase::Nickname;
        } else {
            match self.view.phase {
                PhoneEntryPhase::CheckingAccount => {
                    self.view.phase = PhoneEntryPhase::CheckFailed;
                    self.view.issue = Some(PhoneEntryIssue::CheckFailed);
                }
                PhoneEntryPhase::SendingCode => {
                    self.view.phase = PhoneEntryPhase::CodeRequired;
                    self.view.issue = Some(if self.send_started {
                        PhoneEntryIssue::SendUnconfirmed
                    } else {
                        PhoneEntryIssue::SendFailed
                    });
                }
                PhoneEntryPhase::Verifying => {
                    self.view.phase = PhoneEntryPhase::CodeRequired;
                    self.view.issue = Some(PhoneEntryIssue::VerificationUnconfirmed);
                }
                _ => {}
            }
        }
    }
}
async fn bounded<T>(
    cancel: watch::Sender<bool>,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    if *cancel.borrow() {
        return Err(Error::StaleResponse);
    }
    let mut changed = cancel.subscribe();
    tokio::select! {
        biased;
        _=changed.changed()=>Err(Error::StaleResponse),
        result=tokio::time::timeout(std::time::Duration::from_secs(45),work)=>result.unwrap_or_else(|_|Err(Error::Transport("phone entry operation timeout".into()))),
    }
}
impl CredentialLogin {
    pub fn phone_entry_snapshot(&self) -> Result<Option<PhoneEntryView>> {
        let snapshot = self.lock()?.phone_entry.as_ref().map(|entry| {
            (
                entry.view.clone(),
                entry.registration_started,
                entry.login_started,
            )
        });
        snapshot
            .map(|(mut view, registration_started, login_started)| {
                view.can_reconcile = view.phase == PhoneEntryPhase::RecoveryRequired
                    && (login_started
                        || (registration_started
                            && self.candidate.client().registration_authorized()?));
                Ok(view)
            })
            .transpose()
    }
    fn phone_entry_operation(
        &self,
        id: &str,
        allowed: &[PhoneEntryPhase],
        phase: PhoneEntryPhase,
    ) -> Result<(Operation<'_>, watch::Sender<bool>)> {
        let operation =
            Operation::begin_checked(&self.state, LoginStatus::PhoneEntry, None, |state| {
                let entry = state
                    .phone_entry
                    .as_ref()
                    .filter(|entry| entry.view.id == id)
                    .ok_or(Error::StaleResponse)?;
                if !allowed.contains(&entry.view.phase) {
                    return Err(Error::State("phone entry action is not available"));
                }
                Ok(())
            })?;
        let mut state = operation.lock()?;
        let entry = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
        entry.view.phase = phase;
        entry.view.issue = None;
        entry.view.nickname_error = None;
        let cancel = entry.cancel.clone();
        drop(state);
        Ok((operation, cancel))
    }
    pub async fn phone_entry_start(&self, phone: &str, country: &str) -> Result<()> {
        request::phone_number(phone, country)?;
        let operation =
            Operation::begin_checked(&self.state, LoginStatus::PhoneEntry, None, |state| {
                if state.phone_entry.is_some()
                    || !matches!(state.status, LoginStatus::Ready | LoginStatus::CodeSent)
                {
                    return Err(Error::State("start a new phone entry"));
                }
                Ok(())
            })?;
        let (cancel, _) = watch::channel(false);
        operation.lock()?.phone_entry = Some(Attempt {
            view: PhoneEntryView {
                id: uuid::Uuid::new_v4().to_string(),
                phase: PhoneEntryPhase::CheckingAccount,
                code: None,
                new_user: false,
                can_skip_nickname: false,
                suggested_nickname: None,
                nickname_error: None,
                issue: None,
                registration_blocked: false,
                can_reconcile: false,
            },
            cancel: cancel.clone(),
            phone: phone.into(),
            country: country.into(),
            middle: false,
            verified_sms: None,
            registration_started: false,
            login_started: false,
            send_started: false,
        });
        let result = bounded(cancel, self.phone_entry_check_and_send(&operation)).await;
        self.phone_entry_failed(&operation, &result)?;
        operation.finish(result)
    }
    pub async fn phone_entry_retry(&self, id: &str) -> Result<()> {
        let (operation, cancel) = self.phone_entry_operation(
            id,
            &[PhoneEntryPhase::CheckFailed],
            PhoneEntryPhase::CheckingAccount,
        )?;
        let result = bounded(cancel, self.phone_entry_check_and_send(&operation)).await;
        self.phone_entry_failed(&operation, &result)?;
        operation.finish(result)
    }
    async fn phone_entry_check_and_send(&self, operation: &Operation<'_>) -> Result<()> {
        let middle = self.phone_sms_middle().await?;
        operation.check()?;
        let (phone, country) = {
            let state = operation.lock()?;
            let entry = state.phone_entry.as_ref().ok_or(Error::StaleResponse)?;
            (entry.phone.clone(), entry.country.clone())
        };
        let response = self
            .send(
                operation,
                LoginRequest {
                    path: "/api/cellphone/existence/check",
                    payload: json!({"cellphone":phone,"ctcode":country}),
                },
                false,
                Some(LoginStatus::PhoneEntry),
            )
            .await?;
        if !super::registration::accepted(&response) {
            let mut state = operation.lock()?;
            state.status = LoginStatus::PhoneEntry;
            let entry = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
            entry.view.phase = PhoneEntryPhase::CheckFailed;
            entry.view.code = response["code"].as_i64();
            entry.view.issue = Some(PhoneEntryIssue::CheckFailed);
            return Ok(());
        }
        self.candidate
            .bind_registration_phone(Some((&phone, &country)))?;
        let blocked = self.candidate.registration_pending()?;
        {
            let mut state = operation.lock()?;
            let entry = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
            entry.middle = middle;
            entry.view.new_user = response["exist"] == -1;
            entry.view.can_skip_nickname = response["canSkipSetNickname"] == true;
            entry.view.registration_blocked = blocked;
        }
        self.phone_entry_send(operation).await
    }
    pub async fn phone_entry_resend(&self, id: &str) -> Result<()> {
        let (operation, cancel) = self.phone_entry_operation(
            id,
            &[PhoneEntryPhase::CodeRequired],
            PhoneEntryPhase::SendingCode,
        )?;
        let result = bounded(cancel, self.phone_entry_send(&operation)).await;
        self.phone_entry_failed(&operation, &result)?;
        operation.finish(result)
    }
    async fn phone_entry_send(&self, operation: &Operation<'_>) -> Result<()> {
        let (phone, country, middle) = {
            let mut state = operation.lock()?;
            let entry = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
            entry.view.phase = PhoneEntryPhase::SendingCode;
            entry.verified_sms = None;
            entry.send_started = false;
            (entry.phone.clone(), entry.country.clone(), entry.middle)
        };
        if !self.cooldown.remaining()?.is_zero() {
            let mut state = operation.lock()?;
            state.status = LoginStatus::PhoneEntry;
            let entry = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
            entry.view.phase = PhoneEntryPhase::CodeRequired;
            entry.view.issue = Some(PhoneEntryIssue::Cooldown);
            return Ok(());
        }
        self.cooldown.reserve()?;
        self.check_login_base()?;
        operation
            .lock()?
            .phone_entry
            .as_mut()
            .ok_or(Error::StaleResponse)?
            .send_started = true;
        let response=self.send(operation,LoginRequest {
            path:if middle {"/api/middle/captcha/sent/v1"}else{"/api/sms/captcha/sent"},
            payload:json!({"cellphone":phone,"ctcode":country,"scene":0,"secrete":"music_middleuser_pclogin"})
        },false,Some(LoginStatus::PhoneEntry)).await?;
        let mut state = operation.lock()?;
        state.status = LoginStatus::PhoneEntry;
        let entry = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
        entry.view.phase = PhoneEntryPhase::CodeRequired;
        entry.view.code = response["code"].as_i64();
        entry.view.issue = if super::registration::accepted(&response) {
            None
        } else {
            Some(match response["message"].as_str() {
                Some("当天发送验证码的条数超过限制") => PhoneEntryIssue::DailyLimit,
                Some("发送验证码超过限制:每个手机号一天只能发5条验证码") => {
                    PhoneEntryIssue::FivePerDay
                }
                _ => PhoneEntryIssue::SendFailed,
            })
        };
        Ok(())
    }
    fn phone_entry_failed<T>(&self, operation: &Operation<'_>, result: &Result<T>) -> Result<()> {
        if result.is_ok() {
            return Ok(());
        }
        let mut state = operation.lock()?;
        if let Some(entry) = &mut state.phone_entry {
            entry.interrupted();
            if matches!(
                result.as_ref().err().map(Error::root),
                Some(Error::RequestNotSent(_))
            ) {
                if entry.login_started {
                    entry.login_started = false;
                    entry.view.phase = PhoneEntryPhase::CodeRequired;
                    entry.view.issue = Some(PhoneEntryIssue::LoginRejected);
                }
                if entry.view.issue == Some(PhoneEntryIssue::SendUnconfirmed) {
                    entry.view.issue = Some(PhoneEntryIssue::SendFailed);
                }
            }
            entry.view.can_reconcile = entry.login_started
                || (entry.registration_started
                    && self.candidate.client().registration_authorized()?);
        }
        Ok(())
    }
}
