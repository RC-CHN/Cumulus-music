//! Official uplink/manual SMS and downlink/OTP verification. Candidate-owned;
//! choosing a method never sends a carrier SMS or requests a downlink OTP.
mod model;
mod uplink;
use super::{CredentialLogin, LoginProtocol, LoginStatus, request::LoginRequest, state::Operation};
use crate::{Error, Result};
use model::text;
pub(crate) use model::{Attempt, Context};
pub use model::{SmsDirection, SmsPhase, SmsVerificationView};
use serde_json::json;

impl CredentialLogin {
    pub fn sms_snapshot(&self) -> Result<Option<SmsVerificationView>> {
        Ok(self.lock()?.sms.as_ref().map(|a| a.view.clone()))
    }
    /// Local stop only (M1368 stopUpSmsPolling); no remote cancel endpoint.
    pub fn sms_stop(&self, id: &str) -> Result<()> {
        let mut state = self.lock()?;
        let item = state
            .sms
            .as_ref()
            .filter(|a| a.view.id == id)
            .ok_or(Error::StaleResponse)?;
        item.cancel.send_replace(true);
        state.generation = crate::account::session::next_generation(state.generation)?;
        state.sms = None;
        state.status = LoginStatus::TwoFactor;
        state.busy = false;
        Ok(())
    }
    fn sms_operation(
        &self,
        id: &str,
        direction: SmsDirection,
        reserve: bool,
    ) -> Result<Operation<'_>> {
        let generation = {
            let state = self.lock()?;
            let item = state
                .sms
                .as_ref()
                .filter(|a| a.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if state.pending.is_none()
                || state.verification != Some(LoginStatus::TwoFactor)
                || item.view.direction != direction
                || !matches!(item.view.phase, SmsPhase::Ready | SmsPhase::CodeSent)
            {
                return Err(Error::State("SMS verification is not ready"));
            }
            state.generation
        };
        if reserve {
            self.cooldown.reserve()?;
        }
        Operation::begin(&self.state, LoginStatus::Verifying, Some(generation))
    }
    pub async fn sms_start(&self, direction: SmsDirection) -> Result<()> {
        if self.lock()?.sms.is_some() {
            return Err(Error::State("stop the existing SMS verification first"));
        }
        self.sms_begin(direction, 0).await
    }
    pub async fn sms_refresh(&self, id: &str) -> Result<()> {
        let count = {
            let mut state = self.lock()?;
            let item = state
                .sms
                .as_ref()
                .filter(|a| a.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if state.busy || item.view.direction != SmsDirection::Uplink {
                return Err(Error::State("SMS verification cannot refresh now"));
            }
            let count = item
                .refresh_count
                .checked_add(1)
                .ok_or(Error::State("SMS refresh limit"))?;
            item.cancel.send_replace(true);
            state.sms = None;
            count
        };
        self.sms_begin(SmsDirection::Uplink, count).await
    }
    async fn sms_begin(&self, direction: SmsDirection, refresh_count: u64) -> Result<()> {
        let generation = {
            let state = self.lock()?;
            let context = state
                .sms_context
                .as_ref()
                .filter(|c| c.allowed)
                .ok_or(Error::State("SMS was not offered"))?;
            if state.pending.is_none() || state.verification != Some(LoginStatus::TwoFactor) {
                return Err(Error::State("no SMS challenge pending"));
            }
            if direction == SmsDirection::Uplink && context.country_code.as_deref() != Some("86") {
                return Err(Error::State(
                    "uplink SMS requires the offered 86 country code",
                ));
            }
            state.generation
        };
        let operation = Operation::begin(&self.state, LoginStatus::Verifying, Some(generation))?;
        let cellphone = {
            let mut state = operation.lock()?;
            let context = state.sms_context.as_ref().ok_or(Error::StaleResponse)?;
            let phone_mask = context.phone_mask.clone();
            let country_code = context.country_code.clone();
            let (cancel, _) = tokio::sync::watch::channel(false);
            state.sms = Some(Attempt {
                refresh_count,
                cancel,
                view: SmsVerificationView {
                    id: uuid::Uuid::new_v4().to_string(),
                    direction,
                    phase: if direction == SmsDirection::Uplink {
                        SmsPhase::Loading
                    } else {
                        SmsPhase::Ready
                    },
                    code: None,
                    phone_mask: phone_mask.clone(),
                    country_code,
                    upload_text: None,
                    upload_number: None,
                    answer_rejected: false,
                },
            });
            phone_mask.unwrap_or_default()
        };
        let mut cancel = operation
            .lock()?
            .sms
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .cancel
            .subscribe();
        let work = async {
            if direction == SmsDirection::Uplink {
                let payload = if self.protocol == LoginProtocol::Jsk {
                    json!({"cellphone":cellphone,"enUserId":"undefined","channel":"music_test","scene":"undefined"})
                } else {
                    json!({"channel":"music_test"})
                };
                let response = self
                    .send(
                        &operation,
                        LoginRequest {
                            path: "/api/middle/account/uplinkcaptcha/get",
                            payload,
                        },
                        false,
                        Some(LoginStatus::TwoFactor),
                    )
                    .await?;
                let mut state = operation.lock()?;
                let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
                item.view.code = response["code"].as_i64();
                item.view.phase = SmsPhase::Failed;
                if item.view.code == Some(200) && response["data"]["success"] != false {
                    let data = &response["data"];
                    let number = &data[if refresh_count.is_multiple_of(2) {
                        "uploadNumber"
                    } else {
                        "uploadNumberBackup"
                    }];
                    let number = text(number)
                        .filter(|s| s.len() <= 32 && s.bytes().all(|c| c.is_ascii_digit()))
                        .ok_or(Error::Response("invalid SMS destination"))?;
                    let upload_text =
                        text(&data["uploadText"]).ok_or(Error::Response("invalid SMS text"))?;
                    item.view.upload_text = Some(upload_text);
                    item.view.upload_number = Some(number);
                    item.view.phase = SmsPhase::Ready;
                }
            } else {
                operation.lock()?.status = LoginStatus::TwoFactor;
            }
            Ok(())
        };
        let result = tokio::select! {
            biased;
            _ = cancel.changed() => Err(Error::StaleResponse),
            result = work => result,
        };
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.sms
        {
            item.view.phase = SmsPhase::RecoveryRequired;
        }
        operation.finish(result)
    }
    pub async fn sms_send_down(&self) -> Result<()> {
        self.sms_down(None).await.map(|_| ())
    }
    pub async fn sms_verify_down(&self, code: &str) -> Result<bool> {
        if !(4..=8).contains(&code.len()) || !code.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidInput("invalid SMS verification code"));
        }
        self.sms_down(Some(code)).await
    }
    async fn sms_down(&self, code: Option<&str>) -> Result<bool> {
        let id = self
            .sms_snapshot()?
            .ok_or(Error::State("choose SMS verification first"))?
            .id;
        let operation = self.sms_operation(&id, SmsDirection::Downlink, code.is_none())?;
        {
            let mut state = operation.lock()?;
            let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
            item.view.answer_rejected = false;
            item.view.code = None;
        }
        let mut cancel = operation
            .lock()?
            .sms
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .cancel
            .subscribe();
        let work = async {
            let payload = match (self.protocol, code) {
                (LoginProtocol::Jsk, Some(code)) => {
                    json!({"captcha":code,"ctcode":"undefined","scene":"undefined"})
                }
                (LoginProtocol::Jsk, None) => json!({"scene":"undefined"}),
                (LoginProtocol::Legacy, Some(code)) => json!({"captcha":code}),
                (LoginProtocol::Legacy, None) => json!({}),
            };
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: if code.is_some() {
                            "/api/middle/account/captcha/verify"
                        } else {
                            "/api/middle/account/captcha/sent"
                        },
                        payload,
                    },
                    false,
                    Some(LoginStatus::TwoFactor),
                )
                .await?;
            let accepted = response["code"] == 200
                && response["data"]["success"] != false
                && response["data"] != false;
            {
                let mut state = operation.lock()?;
                let item = state.sms.as_mut().ok_or(Error::StaleResponse)?;
                item.view.code = response["code"].as_i64();
                item.view.answer_rejected = code.is_some() && !accepted;
                item.view.phase = if code.is_none() && accepted {
                    SmsPhase::CodeSent
                } else {
                    SmsPhase::Ready
                };
            }
            if accepted && code.is_some() {
                // From this point a lost identity/replay result must not cause
                // another OTP submission. Only explicit read reconciliation.
                operation
                    .lock()?
                    .sms
                    .as_mut()
                    .ok_or(Error::StaleResponse)?
                    .view
                    .phase = SmsPhase::RecoveryRequired;
                self.complete_verification(&operation).await?;
            }
            Ok(accepted && code.is_some())
        };
        let result = tokio::select! {
            biased;
            _ = cancel.changed() => Err(Error::StaleResponse),
            result = work => result,
        };
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.sms
        {
            item.view.phase = SmsPhase::RecoveryRequired;
        }
        operation.finish(result)
    }
}
