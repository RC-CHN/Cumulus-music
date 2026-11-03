//! Official two-step phone-reuse consent. Only an explicit final action writes.
mod model;
mod registration;
use super::{CredentialLogin, LoginStatus, request::LoginRequest, state::Operation};
use crate::{Error, Result};
pub use model::{RegistrationBlocked, ReuseChoice, ReusePhase, ReuseVerificationView};
use serde_json::{Value, json};

pub(super) struct Attempt {
    pub view: ReuseVerificationView,
    pub cancel: tokio::sync::watch::Sender<bool>,
    pub registration_started: bool,
}
pub(super) async fn bounded<T>(
    cancel: tokio::sync::watch::Sender<bool>,
    work: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    if *cancel.borrow() {
        return Err(Error::StaleResponse);
    }
    let mut cancelled = cancel.subscribe();
    tokio::select! {
        biased;
        _=cancelled.changed()=>Err(Error::StaleResponse),
        result=tokio::time::timeout(std::time::Duration::from_secs(45),work)=>result.unwrap_or_else(|_|Err(Error::Transport("phone reuse operation timeout".into()))),
    }
}
pub(super) fn accepted(response: &Value) -> bool {
    super::registration::accepted(response)
}
fn new_challenge(response: &Value) -> bool {
    matches!(
        super::classify(response),
        LoginStatus::Verification
            | LoginStatus::Captcha
            | LoginStatus::TwoFactor
            | LoginStatus::PhoneReuse
            | LoginStatus::SecurityQuestion
            | LoginStatus::SecurityQr
            | LoginStatus::UpgradeRequired
            | LoginStatus::RiskNotice
            | LoginStatus::AccountDisabled
    ) || response["code"] == 8831
}
impl CredentialLogin {
    pub fn reuse_snapshot(&self) -> Result<Option<ReuseVerificationView>> {
        Ok(self.lock()?.reuse.as_ref().map(|a| a.view.clone()))
    }
    pub async fn reuse_read(&self) -> Result<Value> {
        self.reuse_read_inner(None).await
    }
    pub async fn reuse_back(&self, id: &str) -> Result<Value> {
        self.reuse_read_inner(Some(id)).await
    }
    async fn reuse_read_inner(&self, back: Option<&str>) -> Result<Value> {
        let operation =
            Operation::begin_checked(&self.state, LoginStatus::LoadingVerification, None, |s| {
                if s.pending.is_none() || s.verification != Some(LoginStatus::PhoneReuse) {
                    return Err(Error::State("no phone reuse challenge pending"));
                }
                match (&s.reuse, back) {
                    (None, None) => Ok(()),
                    (Some(a), None) if a.view.phase == ReusePhase::ReadFailed => Ok(()),
                    (Some(a), Some(id))
                        if a.view.id == id
                            && matches!(
                                a.view.phase,
                                ReusePhase::LoginConsent
                                    | ReusePhase::RegisterConsent
                                    | ReusePhase::Nickname
                                    | ReusePhase::LoginFailed
                            ) =>
                    {
                        Ok(())
                    }
                    _ => Err(Error::State(
                        "phone reuse read requires an explicit available step",
                    )),
                }
            })?;
        let (cancel, _) = tokio::sync::watch::channel(false);
        let can_skip = {
            let s = operation.lock()?;
            s.reuse.as_ref().map_or_else(
                || {
                    s.challenge.as_ref().is_some_and(|r| {
                        r["canSkipSetNickname"] == true || r["data"]["canSkipSetNickname"] == true
                    })
                },
                |a| a.view.can_skip_nickname,
            )
        };
        operation.lock()?.reuse = Some(Attempt {
            cancel: cancel.clone(),
            registration_started: false,
            view: ReuseVerificationView {
                id: uuid::Uuid::new_v4().to_string(),
                phase: ReusePhase::Loading,
                code: None,
                masked_nickname: None,
                created_at: None,
                registration_blocked: None,
                can_skip_nickname: can_skip,
                suggested_nickname: None,
                nickname_error: None,
            },
        });
        let result = bounded(cancel, async {
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/login/cellphone/twice/used/user/get",
                        payload: json!({}),
                    },
                    false,
                    Some(LoginStatus::PhoneReuse),
                )
                .await?;
            if new_challenge(&response) {
                operation.lock()?.reuse = None;
                return Ok(response);
            }
            let blocked = if !self.has_registration_sms(&operation)? {
                Some(RegistrationBlocked::FreshSmsRequired)
            } else if self.candidate.registration_pending()? {
                Some(RegistrationBlocked::UnconfirmedRegistration)
            } else {
                None
            };
            let mut state = operation.lock()?;
            let a = state.reuse.as_mut().ok_or(Error::StaleResponse)?;
            a.view.code = response["code"].as_i64();
            if accepted(&response) && response["data"].is_object() {
                a.view.phase = ReusePhase::Choice;
                a.view.masked_nickname = model::masked(&response["data"]["nickName"]);
                a.view.created_at = model::created(&response["data"]["createTime"]);
                a.view.registration_blocked = blocked;
            } else {
                a.view.phase = ReusePhase::ReadFailed;
            }
            Ok(response)
        })
        .await;
        if result.is_err() {
            self.reuse_phase(&operation, ReusePhase::ReadFailed)?;
        }
        operation.finish(result)
    }
    pub fn reuse_choose(&self, id: &str, choice: ReuseChoice) -> Result<()> {
        let mut s = self.lock()?;
        if s.busy || s.verification != Some(LoginStatus::PhoneReuse) {
            return Err(Error::State("phone reuse is unavailable"));
        }
        let a = s
            .reuse
            .as_mut()
            .filter(|a| a.view.id == id)
            .ok_or(Error::StaleResponse)?;
        if a.view.phase != ReusePhase::Choice {
            return Err(Error::State("phone reuse choice is unavailable"));
        }
        a.view.phase = match choice {
            ReuseChoice::Login => ReusePhase::LoginConsent,
            ReuseChoice::Register => ReusePhase::RegisterConsent,
        };
        Ok(())
    }
    fn reuse_operation(
        &self,
        id: &str,
        allowed: &[ReusePhase],
        phase: ReusePhase,
    ) -> Result<(Operation<'_>, tokio::sync::watch::Sender<bool>)> {
        let operation = Operation::begin_checked(&self.state, LoginStatus::Verifying, None, |s| {
            let a = s
                .reuse
                .as_ref()
                .filter(|a| a.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if s.verification != Some(LoginStatus::PhoneReuse) || !allowed.contains(&a.view.phase) {
                return Err(Error::State("phone reuse step is unavailable"));
            }
            Ok(())
        })?;
        let cancel = {
            let mut s = operation.lock()?;
            let a = s.reuse.as_mut().ok_or(Error::StaleResponse)?;
            a.view.phase = phase;
            a.view.code = None;
            a.cancel.clone()
        };
        Ok((operation, cancel))
    }
    fn reuse_phase(&self, operation: &Operation<'_>, phase: ReusePhase) -> Result<()> {
        if let Some(a) = &mut operation.lock()?.reuse {
            a.view.phase = phase;
        }
        Ok(())
    }
    pub async fn reuse_confirm(&self, id: &str) -> Result<()> {
        let (operation, cancel) = self.reuse_operation(
            id,
            &[ReusePhase::LoginConsent, ReusePhase::LoginFailed],
            ReusePhase::Confirming,
        )?;
        let result = bounded(cancel, async {
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/login/cellphone/twice/used/confirm",
                        payload: json!({}),
                    },
                    false,
                    Some(LoginStatus::PhoneReuse),
                )
                .await?;
            if new_challenge(&response) {
                operation.lock()?.reuse = None;
                return Ok(());
            }
            let success = accepted(&response);
            {
                let mut s = operation.lock()?;
                let a = s.reuse.as_mut().ok_or(Error::StaleResponse)?;
                a.view.code = response["code"].as_i64();
                a.view.phase = if success {
                    ReusePhase::RecoveryRequired
                } else {
                    ReusePhase::LoginFailed
                };
            }
            if success {
                self.complete_verification(&operation).await?;
            }
            Ok(())
        })
        .await;
        if let Err(e) = &result {
            let confirmation_not_sent = matches!(e.root(), Error::RequestNotSent(_))
                && operation
                    .lock()?
                    .reuse
                    .as_ref()
                    .is_some_and(|a| a.view.phase == ReusePhase::Confirming);
            self.reuse_phase(
                &operation,
                if confirmation_not_sent {
                    ReusePhase::LoginFailed
                } else {
                    ReusePhase::RecoveryRequired
                },
            )?;
        }
        operation.finish(result)
    }
    pub async fn reuse_reconcile(&self) -> Result<super::LoginSnapshot> {
        let cancel = {
            let s = self.lock()?;
            let a = s
                .reuse
                .as_ref()
                .filter(|a| a.view.phase == ReusePhase::RecoveryRequired)
                .ok_or(Error::State("no unresolved phone reuse operation"))?;
            if a.registration_started && !self.candidate.client().registration_authorized()? {
                return Err(Error::State(
                    "registration did not return authorization; fresh SMS login required",
                ));
            }
            a.cancel.clone()
        };
        bounded(cancel, self.reconcile()).await
    }
}
