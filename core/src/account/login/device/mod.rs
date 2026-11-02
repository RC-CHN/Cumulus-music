//! Original-device confirmation and scan verification, owned by one login candidate.
mod model;
use super::{CredentialLogin, LoginProtocol, LoginStatus, request::LoginRequest, state::Operation};
use crate::{Error, Result};
pub(crate) use model::Attempt;
pub use model::{DeviceVerificationKind, DeviceVerificationPhase, DeviceVerificationView};
use serde_json::{Value, json};

pub(crate) struct DeviceCancellation {
    api: std::sync::Arc<dyn crate::protocol::api::Api>,
    kind: DeviceVerificationKind,
    token: String,
}
impl DeviceCancellation {
    pub(crate) async fn send(self) -> Result<()> {
        self.api
            .call(
                self.kind.path("cancel"),
                json!({"pollingToken":self.token}),
                false,
                &[200],
            )
            .await
            .map(|_| ())
    }
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 256 && !s.chars().any(char::is_control))
        .map(str::to_owned)
}
impl CredentialLogin {
    pub(crate) fn take_device_cleanup(&self) -> Result<Option<DeviceCancellation>> {
        if self.protocol == LoginProtocol::Jsk {
            return Ok(None);
        }
        let mut state = self.lock()?;
        Ok(state.device.as_mut().and_then(|item| {
            item.token.take().map(|token| DeviceCancellation {
                api: self.api.clone(),
                kind: item.view.kind,
                token,
            })
        }))
    }
    pub fn device_snapshot(&self) -> Result<Option<DeviceVerificationView>> {
        Ok(self.lock()?.device.as_ref().map(|v| v.view.clone()))
    }
    fn device_operation(&self, id: &str, cancelling: bool) -> Result<Operation<'_>> {
        let generation = {
            let state = self.lock()?;
            let item = state
                .device
                .as_ref()
                .filter(|v| v.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if state.verification != Some(LoginStatus::TwoFactor)
                || state.pending.is_none()
                || (!cancelling && !item.view.phase.active())
            {
                return Err(Error::State("device verification is not pending"));
            }
            state.generation
        };
        Operation::begin(&self.state, LoginStatus::Verifying, Some(generation))
    }
    fn device_payload(&self, mut value: Value, status_check: bool) -> Value {
        if self.protocol == LoginProtocol::Jsk {
            // M/1368 + M/48 preserve undefined as a form string. Only the
            // confirmation handler stores currentScene as null for status polling.
            value["scene"] = if status_check {
                Value::Null
            } else {
                json!("undefined")
            };
        }
        value
    }
    pub async fn device_start(&self, kind: DeviceVerificationKind, name: &str) -> Result<()> {
        if text(&json!(name)).is_none() {
            return Err(Error::InvalidInput("invalid native device name"));
        }
        let generation = {
            let state = self.lock()?;
            if state.status != LoginStatus::TwoFactor || state.pending.is_none() {
                return Err(Error::State("two-factor verification is not pending"));
            }
            if state.device.as_ref().is_some_and(|v| {
                !matches!(
                    v.view.phase,
                    DeviceVerificationPhase::Cancelled
                        | DeviceVerificationPhase::Completed
                        | DeviceVerificationPhase::Stopped
                        | DeviceVerificationPhase::StopUnconfirmed
                )
            }) {
                return Err(Error::State("stop the existing device verification first"));
            }
            state.generation
        };
        let operation = Operation::begin(&self.state, LoginStatus::Verifying, Some(generation))?;
        operation.lock()?.device = Some(Attempt {
            token: None,
            uncertain_submit: false,
            view: DeviceVerificationView {
                id: uuid::Uuid::new_v4().to_string(),
                kind,
                phase: DeviceVerificationPhase::Starting,
                code: None,
                device_name: None,
                safe_device_count: None,
                modules: None,
                answer_rejected: false,
                poll_after_ms: if self.protocol == LoginProtocol::Jsk {
                    if kind == DeviceVerificationKind::Scan {
                        1000
                    } else {
                        2000
                    }
                } else {
                    3000
                },
            },
        });
        let result = async {
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: kind.path("start"),
                        payload: self.device_payload(json!({"deviceName":name}), false),
                    },
                    false,
                    Some(LoginStatus::TwoFactor),
                )
                .await?;
            let mut state = operation.lock()?;
            let item = state.device.as_mut().ok_or(Error::StaleResponse)?;
            item.view.code = response["code"].as_i64();
            item.view.phase = match item.view.code {
                Some(200) => DeviceVerificationPhase::Waiting,
                Some(815) => DeviceVerificationPhase::Blocked,
                _ => DeviceVerificationPhase::Error,
            };
            if item.view.code == Some(200) {
                let token = response["data"]["pollingToken"]
                    .as_str()
                    .filter(|s| {
                        !s.is_empty() && s.len() <= 2048 && !s.chars().any(char::is_control)
                    })
                    .ok_or(Error::Response("missing device verification token"))?;
                item.token = Some(token.into());
                item.view.device_name = text(&response["data"]["deviceName"]);
                item.view.safe_device_count = response["data"]["safeDeviceCount"]
                    .as_u64()
                    .filter(|n| *n <= 10000);
                if kind == DeviceVerificationKind::Scan {
                    // Official DoubleVerifyScan.tsx QR destination. Never export its URL or token.
                    let mut url = url::Url::parse("orpheus://rnpage")
                        .map_err(|_| Error::Protocol("device QR URL"))?;
                    url.query_pairs_mut()
                        .append_pair("component", "rn-account-verify")
                        .append_pair("isTheme", "true")
                        .append_pair("immersiveMode", "true")
                        .append_pair("route", "confirmOldDevice")
                        .append_pair("pollingToken", token);
                    let qr =
                        qrcodegen::QrCode::encode_text(url.as_str(), qrcodegen::QrCodeEcc::Low)
                            .map_err(|_| Error::Response("device QR exceeds size limit"))?;
                    item.view.modules = Some(
                        (0..qr.size())
                            .map(|y| (0..qr.size()).map(|x| qr.get_module(x, y)).collect())
                            .collect(),
                    );
                }
            }
            Ok(())
        }
        .await;
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.device
        {
            item.view.phase = DeviceVerificationPhase::RecoveryRequired;
        }
        operation.finish(result)
    }
    pub async fn device_poll(&self, id: &str) -> Result<bool> {
        self.device_request(id, None).await
    }
    pub async fn device_verify(&self, id: &str, code: &str) -> Result<bool> {
        if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidInput("invalid device confirmation code"));
        }
        self.device_request(id, Some(code)).await
    }
    async fn device_request(&self, id: &str, code: Option<&str>) -> Result<bool> {
        if code.is_some()
            && self
                .lock()?
                .device
                .as_ref()
                .is_some_and(|v| v.view.id == id && v.view.kind != DeviceVerificationKind::Confirm)
        {
            return Err(Error::InvalidInput(
                "scan verification has no confirmation code",
            ));
        }
        if code.is_some()
            && self
                .lock()?
                .device
                .as_ref()
                .is_some_and(|v| v.view.id == id && v.uncertain_submit)
        {
            return Err(Error::MutationUncertain);
        }
        let operation = self.device_operation(id, false)?;
        let result = async {
            let (kind, token) = {
                let state = operation.lock()?;
                let item = state.device.as_ref().ok_or(Error::StaleResponse)?;
                if code.is_some() && item.view.kind != DeviceVerificationKind::Confirm {
                    return Err(Error::InvalidInput(
                        "scan verification has no confirmation code",
                    ));
                }
                (
                    item.view.kind,
                    item.token
                        .clone()
                        .ok_or(Error::State("device request outcome is unknown"))?,
                )
            };
            let mut payload = self.device_payload(
                json!({"pollingToken":token}),
                code.is_none() && kind == DeviceVerificationKind::Confirm,
            );
            if let Some(code) = code {
                payload["captcha"] = code.into();
            }
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: kind.path(if code.is_some() { "verify" } else { "poll" }),
                        payload,
                    },
                    false,
                    Some(LoginStatus::TwoFactor),
                )
                .await?;
            let service_code = response["code"].as_i64();
            let completed = service_code == Some(200) && response["data"]["success"] != false;
            {
                let mut state = operation.lock()?;
                let item = state.device.as_mut().ok_or(Error::StaleResponse)?;
                item.view.code = service_code;
                if code.is_some() {
                    item.view.answer_rejected = !completed;
                }
                item.view.phase = match service_code {
                    Some(801) => DeviceVerificationPhase::Waiting,
                    Some(802)
                        if kind == DeviceVerificationKind::Scan
                            || self.protocol == LoginProtocol::Legacy =>
                    {
                        DeviceVerificationPhase::Scanned
                    }
                    Some(200)
                        if completed
                            && kind == DeviceVerificationKind::Confirm
                            && code.is_none() =>
                    {
                        DeviceVerificationPhase::CodeRequired
                    }
                    Some(200) if completed => DeviceVerificationPhase::Completed,
                    Some(810) => DeviceVerificationPhase::Expired,
                    Some(811) => DeviceVerificationPhase::Rejected,
                    Some(815) => DeviceVerificationPhase::Blocked,
                    _ if code.is_some() => DeviceVerificationPhase::Waiting,
                    _ => DeviceVerificationPhase::Error,
                };
                if item.uncertain_submit {
                    item.view.phase = DeviceVerificationPhase::RecoveryRequired;
                }
                if self.protocol == LoginProtocol::Legacy {
                    item.view.poll_after_ms = response["data"]["intervalMillis"]
                        .as_u64()
                        .filter(|n| (100..=60000).contains(n))
                        .unwrap_or(3000);
                }
                if !matches!(
                    item.view.phase,
                    DeviceVerificationPhase::Waiting | DeviceVerificationPhase::Scanned
                ) {
                    item.view.modules = None;
                }
            }
            if completed && (kind == DeviceVerificationKind::Scan || code.is_some()) {
                operation.lock()?.device = None;
                self.complete_verification(&operation).await?;
                return Ok(true);
            }
            Ok(false)
        }
        .await;
        if result.is_err()
            && let Ok(mut state) = operation.lock()
            && let Some(item) = &mut state.device
        {
            item.uncertain_submit |= code.is_some();
            item.view.phase = if code.is_some() {
                DeviceVerificationPhase::RecoveryRequired
            } else {
                DeviceVerificationPhase::Error
            };
        }
        operation.finish(result)
    }
    pub async fn device_cancel(&self, id: &str) -> Result<()> {
        if self.lock()?.device.as_ref().is_some_and(|v| {
            v.view.id == id
                && matches!(
                    v.view.phase,
                    DeviceVerificationPhase::Cancelled | DeviceVerificationPhase::StopUnconfirmed
                )
        }) {
            return Ok(());
        }
        let operation = self.device_operation(id, true)?;
        let (kind, token) = {
            let mut state = operation.lock()?;
            let item = state.device.as_mut().ok_or(Error::StaleResponse)?;
            item.view.modules = None;
            item.view.phase = DeviceVerificationPhase::StopUnconfirmed;
            (item.view.kind, item.token.take())
        };
        let confirmed = if let Some(token) = token {
            self.api
                .request(
                    kind.path("cancel"),
                    self.device_payload(json!({"pollingToken":token}), false),
                    false,
                )
                .await
                .is_ok_and(|r| r["code"] == 200)
        } else {
            false
        };
        {
            let mut state = operation.lock()?;
            state
                .device
                .as_mut()
                .ok_or(Error::StaleResponse)?
                .view
                .phase = if confirmed {
                DeviceVerificationPhase::Cancelled
            } else {
                DeviceVerificationPhase::StopUnconfirmed
            };
            state.status = LoginStatus::TwoFactor;
        }
        operation.finish(Ok(()))
    }
    /// JSK stop*Verify only stops local polling. Legacy Back also cancels remotely.
    /// Revoke this operation before waiting for any previous network response.
    pub async fn device_leave(&self, id: &str) -> Result<()> {
        let (kind, token, generation) = {
            let mut state = self.lock()?;
            if state.verification != Some(LoginStatus::TwoFactor) || state.pending.is_none() {
                return Err(Error::State("device verification is not pending"));
            }
            let item = state
                .device
                .as_mut()
                .filter(|v| v.view.id == id)
                .ok_or(Error::StaleResponse)?;
            if matches!(
                item.view.phase,
                DeviceVerificationPhase::Stopped
                    | DeviceVerificationPhase::Cancelled
                    | DeviceVerificationPhase::StopUnconfirmed
            ) {
                return Ok(());
            }
            let kind = item.view.kind;
            let token = item.token.take();
            item.view.modules = None;
            item.view.phase = if self.protocol == LoginProtocol::Jsk {
                DeviceVerificationPhase::Stopped
            } else {
                DeviceVerificationPhase::StopUnconfirmed
            };
            state.generation = crate::account::session::next_generation(state.generation)?;
            state.status = LoginStatus::TwoFactor;
            state.busy = false;
            (kind, token, state.generation)
        };
        if self.protocol == LoginProtocol::Legacy
            && let Some(token) = token
        {
            let confirmed = self
                .api
                .request(kind.path("cancel"), json!({"pollingToken":token}), false)
                .await
                .is_ok_and(|r| r["code"] == 200);
            let mut state = self.lock()?;
            if state.generation != generation {
                return Err(Error::StaleResponse);
            }
            state
                .device
                .as_mut()
                .filter(|v| v.view.id == id)
                .ok_or(Error::StaleResponse)?
                .view
                .phase = if confirmed {
                DeviceVerificationPhase::Cancelled
            } else {
                DeviceVerificationPhase::StopUnconfirmed
            };
        }
        Ok(())
    }
}
