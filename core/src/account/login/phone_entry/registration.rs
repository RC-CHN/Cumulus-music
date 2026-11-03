use super::*;
use crate::account::login::{PhoneCredential, PhoneLogin, nickname, registration};

impl CredentialLogin {
    pub async fn phone_entry_submit(&self, id: &str, code: &str, remember: bool) -> Result<()> {
        request::sms_code(code)?;
        let (operation, cancel) = self.phone_entry_operation(
            id,
            &[PhoneEntryPhase::CodeRequired],
            PhoneEntryPhase::Verifying,
        )?;
        let result = bounded(cancel, async {
            let (phone, country, new_user) = {
                let state = operation.lock()?;
                let a = state.phone_entry.as_ref().ok_or(Error::StaleResponse)?;
                (a.phone.clone(), a.country.clone(), a.view.new_user)
            };
            self.check_login_base()?;
            self.candidate.remember(remember);
            let blocked = self.candidate.registration_pending()?;
            if !new_user || blocked {
                // A durable registration with an unknown result is only resolved
                // through a fresh explicit SMS login; never replay registration.
                operation
                    .lock()?
                    .phone_entry
                    .as_mut()
                    .ok_or(Error::StaleResponse)?
                    .login_started = true;
                let input = PhoneLogin {
                    phone: &phone,
                    country_code: &country,
                    credential: PhoneCredential::Sms(code),
                    remember,
                    secure_captcha: None,
                };
                let request = match self.protocol {
                    super::super::LoginProtocol::Legacy => request::phone(input)?,
                    super::super::LoginProtocol::Jsk => request::jsk_phone(input)?,
                };
                let response = self.send(&operation, request, true, None).await?;
                let mut state = operation.lock()?;
                if let Some(a) = &mut state.phone_entry {
                    a.login_started = false;
                    a.view.phase = PhoneEntryPhase::CodeRequired;
                    a.view.registration_blocked = blocked;
                    a.view.issue = Some(if response["code"] == 503 {
                        PhoneEntryIssue::CodeInvalid
                    } else {
                        PhoneEntryIssue::LoginRejected
                    });
                    a.view.code = response["code"].as_i64();
                }
                return Ok(());
            }
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/sms/captcha/verify",
                        payload: json!({"cellphone":phone,"captcha":code,"ctcode":country}),
                    },
                    false,
                    Some(LoginStatus::PhoneEntry),
                )
                .await?;
            if !registration::accepted(&response) {
                let mut state = operation.lock()?;
                state.status = LoginStatus::PhoneEntry;
                let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                a.view.phase = PhoneEntryPhase::CodeRequired;
                a.view.code = response["code"].as_i64();
                a.view.issue = Some(PhoneEntryIssue::CodeInvalid);
                return Ok(());
            }
            {
                let mut state = operation.lock()?;
                let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                a.verified_sms = Some(code.into());
                a.view.phase = PhoneEntryPhase::Hinting;
                a.view.issue = None;
            }
            let hint = registration::hint(self.candidate.api().as_ref()).await;
            let mut state = operation.lock()?;
            state.status = LoginStatus::PhoneEntry;
            let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
            a.view.phase = PhoneEntryPhase::Nickname;
            a.view.suggested_nickname = hint;
            Ok(())
        })
        .await;
        self.phone_entry_failed(&operation, &result)?;
        operation.finish(result)
    }

    pub async fn phone_entry_nickname_hint(&self, id: &str) -> Result<()> {
        let (operation, cancel) =
            self.phone_entry_operation(id, &[PhoneEntryPhase::Nickname], PhoneEntryPhase::Hinting)?;
        let result = bounded(cancel, async {
            let hint = registration::hint(self.candidate.api().as_ref()).await;
            let mut state = operation.lock()?;
            let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
            a.view.phase = PhoneEntryPhase::Nickname;
            a.view.suggested_nickname = hint;
            Ok(())
        })
        .await;
        self.phone_entry_failed(&operation, &result)?;
        operation.finish(result)
    }

    pub async fn phone_entry_register(&self, id: &str, name: Option<&str>) -> Result<()> {
        let (operation, cancel) = self.phone_entry_operation(
            id,
            &[PhoneEntryPhase::Nickname],
            PhoneEntryPhase::CheckingNickname,
        )?;
        let mut intent = None;
        let mut submitted = false;
        let result = bounded(cancel, async {
            self.check_login_base()?;
            if self.candidate.registration_pending()? {
                let mut state = operation.lock()?;
                let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                a.view.registration_blocked = true;
                a.view.phase = PhoneEntryPhase::RecoveryRequired;
                return Ok(());
            }
            let mut payload = {
                let state = operation.lock()?;
                let a = state.phone_entry.as_ref().ok_or(Error::StaleResponse)?;
                if !a.view.new_user {
                    return Err(Error::State("phone was not observed as a new account"));
                }
                registration::payload(
                    &a.phone,
                    &a.country,
                    a.verified_sms
                        .as_deref()
                        .ok_or(Error::State("verified SMS is required"))?,
                    false,
                )
            };
            if let Some(name) = name {
                let name = match nickname::validate(name) {
                    Ok(name) => name,
                    Err(error) => {
                        let mut state = operation.lock()?;
                        let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                        a.view.phase = PhoneEntryPhase::Nickname;
                        a.view.nickname_error = Some(error);
                        return Ok(());
                    }
                };
                if let Some(error) = registration::check(self.candidate.api().as_ref(), name).await
                {
                    let mut state = operation.lock()?;
                    let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                    a.view.phase = PhoneEntryPhase::Nickname;
                    a.view.nickname_error = Some(error);
                    return Ok(());
                }
                payload["nickname"] = name.into();
            } else {
                if !operation
                    .lock()?
                    .phone_entry
                    .as_ref()
                    .is_some_and(|a| a.view.can_skip_nickname)
                {
                    return Err(Error::InvalidInput("default nickname was not offered"));
                }
                payload["isSkipSetNickname"] = "true".into();
            }
            operation.check()?;
            self.check_login_base()?;
            intent = Some(self.candidate.registration_begin()?);
            {
                let mut state = operation.lock()?;
                let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                a.registration_started = true;
                a.view.phase = PhoneEntryPhase::Registering;
            }
            submitted = true;
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/w/register/cellphone",
                        payload,
                    },
                    false,
                    Some(LoginStatus::PhoneEntry),
                )
                .await?;
            if !registration::accepted(&response) {
                let rejected = registration::rejection(&response);
                if rejected.is_none() || self.candidate.client().registration_authorized()? {
                    let mut state = operation.lock()?;
                    let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                    a.view.phase = PhoneEntryPhase::RecoveryRequired;
                    a.view.code = response["code"].as_i64();
                    return Ok(());
                }
                self.candidate.registration_not_sent(
                    intent
                        .as_ref()
                        .ok_or(Error::State("registration intent missing"))?,
                )?;
                let mut state = operation.lock()?;
                state.status = LoginStatus::PhoneEntry;
                let a = state.phone_entry.as_mut().ok_or(Error::StaleResponse)?;
                a.registration_started = false;
                a.view.nickname_error = rejected;
                a.view.phase = PhoneEntryPhase::Nickname;
                a.view.code = response["code"].as_i64();
                if matches!(
                    rejected,
                    Some(
                        super::super::NicknameError::SmsInvalid
                            | super::super::NicknameError::PhoneRegistered
                    )
                ) {
                    a.verified_sms = None;
                    a.view.phase = PhoneEntryPhase::CodeRequired;
                    a.view.issue = Some(
                        if rejected == Some(super::super::NicknameError::SmsInvalid) {
                            PhoneEntryIssue::CodeInvalid
                        } else {
                            PhoneEntryIssue::LoginRejected
                        },
                    );
                    if rejected == Some(super::super::NicknameError::PhoneRegistered) {
                        a.view.new_user = false;
                    }
                }
                return Ok(());
            }
            operation
                .lock()?
                .phone_entry
                .as_mut()
                .ok_or(Error::StaleResponse)?
                .view
                .phase = PhoneEntryPhase::RecoveryRequired;
            if !self.candidate.client().registration_authorized()? {
                return Err(Error::State(
                    "registration did not issue authorization; fresh SMS login required",
                ));
            }
            self.verify_identity(&operation).await
        })
        .await;
        if let Err(error) = &result {
            let not_sent = !submitted || matches!(error.root(), Error::RequestNotSent(_));
            if not_sent && let Some(intent) = &intent {
                self.candidate.registration_not_sent(intent)?;
                if let Ok(mut state) = operation.lock()
                    && let Some(a) = &mut state.phone_entry
                {
                    a.registration_started = false;
                }
            }
        }
        self.phone_entry_failed(&operation, &result)?;
        operation.finish(result)
    }

    pub async fn phone_entry_reconcile(&self) -> Result<super::super::LoginSnapshot> {
        let snapshot = self
            .phone_entry_snapshot()?
            .ok_or(Error::State("phone entry is not active"))?;
        if snapshot.phase != PhoneEntryPhase::RecoveryRequired || !snapshot.can_reconcile {
            return Err(Error::State(
                "fresh SMS login is required before reconciling registration",
            ));
        }
        let cancel = self
            .lock()?
            .phone_entry
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .cancel
            .clone();
        bounded(cancel, self.reconcile()).await
    }
}
