use super::{
    LoginProtocol, LoginStatus, PhoneLogin, ResendCooldown, VerificationAction, classify,
    request::{self, LoginRequest},
    state::{Operation, State},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken, profiles::LoginCandidate, session::next_generation},
    protocol::api::{Api, ApiFuture},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, MutexGuard};

/// Actual provider result for the official login checkToken branch.
pub trait LoginGuardian: Send + Sync {
    fn token(&self) -> ApiFuture<'_, String>;
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginSnapshot {
    pub status: LoginStatus,
    pub code: Option<i64>,
    pub busy: bool,
    pub captcha_available: bool,
    pub error_kind: Option<&'static str>,
    pub retry_after_ms: u64,
}

pub struct CredentialLogin {
    session: Arc<AccountSession>,
    candidate: LoginCandidate,
    base: SessionToken,
    pub(super) state: Mutex<State>,
    pub(super) cooldown: Arc<ResendCooldown>,
    guardian: Option<Arc<dyn LoginGuardian>>,
    pub(super) protocol: LoginProtocol,
    pub(super) api: Arc<dyn Api>,
}

impl CredentialLogin {
    pub fn new(
        session: Arc<AccountSession>,
        candidate: LoginCandidate,
        cooldown: Arc<ResendCooldown>,
        guardian: Option<Arc<dyn LoginGuardian>>,
    ) -> Result<Self> {
        Self::new_with_protocol(
            session,
            candidate,
            cooldown,
            guardian,
            LoginProtocol::Legacy,
        )
    }
    pub fn new_with_protocol(
        session: Arc<AccountSession>,
        candidate: LoginCandidate,
        cooldown: Arc<ResendCooldown>,
        guardian: Option<Arc<dyn LoginGuardian>>,
        protocol: LoginProtocol,
    ) -> Result<Self> {
        let base = session.token()?;
        let api = protocol.api(candidate.client());
        Ok(Self {
            session,
            candidate,
            base,
            state: Mutex::new(State::default()),
            cooldown,
            guardian,
            protocol,
            api,
        })
    }
    pub(super) fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }
    pub fn snapshot(&self) -> Result<LoginSnapshot> {
        let state = self.lock()?;
        Ok(LoginSnapshot {
            status: state.status,
            code: state.code,
            busy: state.busy,
            captcha_available: state.status == LoginStatus::Captcha && state.pending.is_some(),
            error_kind: state.error_kind,
            retry_after_ms: self.cooldown.remaining()?.as_millis().min(u64::MAX as u128) as u64,
        })
    }
    /// Native-only challenge data; hosts must construct a safe UI DTO.
    pub fn challenge_native(&self) -> Result<Option<Value>> {
        Ok(self.lock()?.challenge.clone())
    }
    pub fn cancel(&self) -> Result<()> {
        let mut state = self.lock()?;
        if state.status == LoginStatus::Authenticated {
            return Err(Error::State(
                "completed login cannot be cancelled; use logout",
            ));
        }
        state.generation = next_generation(state.generation)?;
        if let Some(sms) = &state.sms {
            sms.cancel.send_replace(true);
        }
        state.status = LoginStatus::Cancelled;
        state.pending = None;
        state.challenge = None;
        state.verification = None;
        state.device = None;
        state.sms = None;
        state.sms_context = None;
        state.busy = false;
        Ok(())
    }

    pub(super) async fn send(
        &self,
        operation: &Operation<'_>,
        request: LoginRequest,
        login: bool,
        preserve: Option<LoginStatus>,
    ) -> Result<Value> {
        let mut payload = request.payload.clone();
        {
            let mut state = operation.lock()?;
            if login {
                if let Some(remember) = request.payload["remember"].as_bool() {
                    self.candidate.remember(remember);
                }
                state.pending = Some(request.clone());
            }
        }
        if login && let Some(guardian) = &self.guardian {
            let token = guardian.token().await?;
            if token.is_empty() || token.contains(['\r', '\n', '\0']) {
                return Err(Error::State("login provider returned no valid token"));
            }
            payload["checkToken"] = token.into();
        }
        operation.check()?;
        // The recovered PC SMS-send UI still uses the native legacy helper.
        // Its SDK bridge producer is not present in M/290; do not invent one.
        let api = if matches!(
            request.path,
            "/api/sms/captcha/sent" | "/api/middle/captcha/sent/v1"
        ) {
            self.candidate.api()
        } else {
            self.api.clone()
        };
        let response = api.request(request.path, payload, false).await;
        {
            let mut state = operation.lock()?;
            let response = response.as_ref().ok();
            // Classify only complete object responses. The actual error is
            // returned below without storing request bodies in diagnostics.
            if let Some(response) = response.filter(|v| v.is_object()) {
                let code = response["code"].as_i64();
                let result = classify(response);
                state.code = code;
                state.status = if login {
                    result
                } else if code == Some(200) {
                    preserve.unwrap_or(LoginStatus::Ready)
                } else if result == LoginStatus::Rejected
                    && preserve.is_some_and(LoginStatus::verification)
                {
                    preserve.unwrap()
                } else {
                    result
                };
                if !matches!(
                    state.status,
                    LoginStatus::Authorized | LoginStatus::Ready | LoginStatus::CodeSent
                ) {
                    state.challenge = Some(response.clone());
                }
                if state.status.verification() {
                    state.verification = Some(state.status);
                }
                if login && state.status != LoginStatus::Authorized {
                    // Keep the active cancellation handle while an acknowledged
                    // SMS step replays login or awaits account/get. Dropping it
                    // before network completion would cancel our own operation.
                    state.sms = None;
                    state.sms_context = None;
                }
                if matches!(code, Some(8830 | 8860))
                    && let Some(ticket) = response["MUSIC_TU"].as_str().filter(|s| !s.is_empty())
                {
                    self.candidate.credentials().set_login_ticket(ticket)?;
                }
            }
        }
        let response = response?;
        if !response.is_object() {
            return Err(Error::Response("login response must be an object"));
        }
        if login && response["code"].as_i64() == Some(200) {
            self.verify_identity(operation).await?;
        }
        Ok(response)
    }

    async fn verify_identity(&self, operation: &Operation<'_>) -> Result<()> {
        operation.check()?;
        let profile = self.candidate.api().account().await;
        let mut state = operation.lock()?;
        self.candidate.adopt(&self.session, &self.base, profile?)?;
        state.pending = None;
        state.challenge = None;
        state.verification = None;
        state.device = None;
        state.sms = None;
        state.sms_context = None;
        state.status = LoginStatus::Authenticated;
        Ok(())
    }

    async fn submit(
        &self,
        request: LoginRequest,
        expected: Option<u64>,
        remember: Option<bool>,
    ) -> Result<Value> {
        let operation = Operation::begin(&self.state, LoginStatus::Submitting, expected)?;
        if let Some(remember) = remember {
            self.candidate.remember(remember);
        }
        let result = self.send(&operation, request, true, None).await;
        operation.finish(result)
    }
    pub async fn phone(&self, input: PhoneLogin<'_>) -> Result<Value> {
        let remember = input.remember;
        let request = match self.protocol {
            LoginProtocol::Legacy => request::phone(input)?,
            LoginProtocol::Jsk => request::jsk_phone(input)?,
        };
        self.submit(request, None, Some(remember)).await
    }
    pub async fn email(
        &self,
        address: &str,
        password: &str,
        remember: bool,
        secure_captcha: Option<&str>,
    ) -> Result<Value> {
        self.submit(
            match self.protocol {
                LoginProtocol::Legacy => {
                    request::email(address, password, remember, secure_captcha)?
                }
                LoginProtocol::Jsk => {
                    let mut request = request::jsk_email(address, password)?;
                    if let Some(captcha) = secure_captcha.filter(|value| !value.is_empty()) {
                        request.payload["secureCaptcha"] = captcha.into();
                    }
                    request
                }
            },
            None,
            Some(remember),
        )
        .await
    }
    pub async fn submit_captcha(&self, value: &str) -> Result<Value> {
        if value.is_empty() {
            return Err(Error::InvalidInput("empty completed CAPTCHA result"));
        }
        let (mut request, generation) = {
            let state = self.lock()?;
            if state.status != LoginStatus::Captcha {
                return Err(Error::State("no CAPTCHA challenge is pending"));
            }
            (
                state
                    .pending
                    .clone()
                    .ok_or(Error::State("missing pending login"))?,
                state.generation,
            )
        };
        request.payload["secureCaptcha"] = value.into();
        self.submit(request, Some(generation), None).await
    }
    /// Call only after an explicit user send action. No automatic retry.
    pub async fn send_sms(&self, phone: &str, country_code: &str, middle: bool) -> Result<Value> {
        request::phone_number(phone, country_code)?;
        let operation = Operation::begin(&self.state, LoginStatus::SendingSms, None)?;
        let result = async {
            self.cooldown.reserve()?;
            self.send(
                &operation,
                LoginRequest {
                    path: if middle {
                        "/api/middle/captcha/sent/v1"
                    } else {
                        "/api/sms/captcha/sent"
                    },
                    payload: json!({"cellphone":phone,"ctcode":country_code}),
                },
                false,
                Some(LoginStatus::CodeSent),
            )
            .await
        }
        .await;
        operation.finish(result)
    }
    pub async fn verification_methods(&self) -> Result<Value> {
        let generation = {
            let state = self.lock()?;
            if state.verification != Some(LoginStatus::TwoFactor) {
                return Err(Error::State("two-factor verification is not pending"));
            }
            state.generation
        };
        let operation = Operation::begin(
            &self.state,
            LoginStatus::LoadingVerification,
            Some(generation),
        )?;
        let result = async {
            if self.protocol == LoginProtocol::Jsk {
                // M/1368 beginVerify precedes the list and swallows its own
                // network/service failure. Only the read follows; never resend.
                let _ = self
                    .send(
                        &operation,
                        LoginRequest {
                            path: "/api/user/2fa/begin",
                            payload: json!({}),
                        },
                        false,
                        Some(LoginStatus::TwoFactor),
                    )
                    .await;
                operation.check()?;
            }
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/user/2fa/list",
                        payload: json!({"bindingCellphoneEnable":true,"smsCaptchaAble":true}),
                    },
                    false,
                    Some(LoginStatus::TwoFactor),
                )
                .await?;
            if response["code"].as_i64() == Some(200) && self.protocol == LoginProtocol::Legacy {
                let begun = self
                    .send(
                        &operation,
                        LoginRequest {
                            path: "/api/user/2fa/begin",
                            payload: json!({}),
                        },
                        false,
                        Some(LoginStatus::TwoFactor),
                    )
                    .await?;
                if begun["code"].as_i64() != Some(200) {
                    return Err(Error::service(
                        "/api/user/2fa/begin",
                        begun["code"].clone(),
                        begun,
                    ));
                }
            }
            if response["code"] == 200 {
                operation.lock()?.sms_context = Some(super::sms::Context::from_list(&response));
            }
            Ok(response)
        }
        .await;
        operation.finish(result)
    }
    pub async fn verification(&self, action: VerificationAction, params: Value) -> Result<Value> {
        let mut request = action.request(params)?;
        if matches!(action, VerificationAction::SmsSend) {
            request.payload = if self.protocol == LoginProtocol::Jsk {
                json!({"scene":"undefined"})
            } else {
                json!({})
            };
        } else if matches!(action, VerificationAction::SmsVerify)
            && self.protocol == LoginProtocol::Jsk
        {
            request.payload["ctcode"] = json!("undefined");
            request.payload["scene"] = json!("undefined");
        }
        let (generation, previous) = {
            let state = self.lock()?;
            if state.pending.is_none() {
                return Err(Error::State("no login verification is pending"));
            }
            (
                state.generation,
                state
                    .verification
                    .ok_or(Error::State("no applicable verification challenge"))?,
            )
        };
        let operation = Operation::begin(&self.state, LoginStatus::Verifying, Some(generation))?;
        let result = async {
            if request.sends_message {
                self.cooldown.reserve()?;
            }
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: request.path,
                        payload: request.payload,
                    },
                    false,
                    Some(previous),
                )
                .await?;
            // M2992 treats an empty question as an acknowledged step requiring
            // the original login again. Read identity first; only a 301 allows
            // that one replay, as for other completed verification steps.
            let no_question = matches!(action, VerificationAction::QuestionGet)
                && (response["data"].is_null()
                    || response["data"].as_object().is_some_and(|v| v.is_empty()));
            if response["code"].as_i64() == Some(200)
                && response["data"]["success"] != false
                && response["data"] != false
                && (request.completes || no_question)
                && let Some(replayed) = self.complete_verification(&operation).await?
            {
                return Ok(replayed);
            }
            Ok(response)
        }
        .await;
        operation.finish(result)
    }
    pub(super) async fn complete_verification(
        &self,
        operation: &Operation<'_>,
    ) -> Result<Option<Value>> {
        match self.verify_identity(operation).await {
            Ok(()) => Ok(None),
            Err(error) if error.is_service_code(301) => {
                let pending = operation
                    .lock()?
                    .pending
                    .clone()
                    .ok_or(Error::State("missing pending login"))?;
                // Only an acknowledged completed verification permits this one replay.
                self.send(operation, pending, true, None).await.map(Some)
            }
            Err(error) => Err(error),
        }
    }
    pub async fn reconcile(&self) -> Result<LoginSnapshot> {
        let operation = Operation::begin(&self.state, LoginStatus::CheckingIdentity, None)?;
        let result = self.verify_identity(&operation).await;
        operation.finish(result)?;
        self.snapshot()
    }
}
