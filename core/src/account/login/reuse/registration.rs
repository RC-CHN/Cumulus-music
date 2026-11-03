use super::*;
impl CredentialLogin {
    fn registration_sms(&self, operation: &Operation<'_>) -> Result<Value> {
        let s = operation.lock()?;
        let p = s
            .pending
            .as_ref()
            .filter(|p| matches!(p.path, "/api/login/cellphone" | "/api/w/login/cellphone"))
            .ok_or(Error::State("fresh SMS login required"))?;
        let phone = p.payload["phone"]
            .as_str()
            .ok_or(Error::State("missing original phone"))?;
        let captcha = p.payload["captcha"]
            .as_str()
            .ok_or(Error::State("fresh SMS login required"))?;
        let country = p
            .payload
            .get("countryCode")
            .or_else(|| p.payload.get("countrycode"))
            .and_then(Value::as_str)
            .ok_or(Error::State("missing original country"))?;
        Ok(super::super::registration::payload(
            phone, country, captcha, true,
        ))
    }
    pub(super) fn has_registration_sms(&self, operation: &Operation<'_>) -> Result<bool> {
        Ok(self.registration_sms(operation).is_ok())
    }
    pub async fn reuse_prepare_register(&self, id: &str) -> Result<()> {
        self.reuse_hint_inner(id, true).await
    }
    pub async fn reuse_nickname_hint(&self, id: &str) -> Result<()> {
        self.reuse_hint_inner(id, false).await
    }
    async fn reuse_hint_inner(&self, id: &str, prepare: bool) -> Result<()> {
        let (operation, cancel) = self.reuse_operation(
            id,
            if prepare {
                &[ReusePhase::RegisterConsent]
            } else {
                &[ReusePhase::Nickname]
            },
            ReusePhase::Hinting,
        )?;
        let result = bounded(cancel, async {
            self.registration_sms(&operation)?;
            if self.candidate.registration_pending()? {
                return Err(Error::State("registration result is unresolved"));
            }
            // The actual PC nickname page uses M3138 -> M15 legacy helpers,
            // including when the preceding phone challenge used JSK.
            let hint = super::super::registration::hint(self.candidate.api().as_ref()).await;
            let mut s = operation.lock()?;
            let a = s.reuse.as_mut().ok_or(Error::StaleResponse)?;
            a.view.phase = ReusePhase::Nickname;
            a.view.suggested_nickname = hint;
            a.view.nickname_error = None;
            Ok(())
        })
        .await;
        if result.is_err() {
            self.reuse_phase(
                &operation,
                if prepare {
                    ReusePhase::RegisterConsent
                } else {
                    ReusePhase::Nickname
                },
            )?;
        }
        operation.finish(result)
    }
    pub async fn reuse_register(&self, id: &str, nickname: Option<&str>) -> Result<()> {
        let (operation, cancel) =
            self.reuse_operation(id, &[ReusePhase::Nickname], ReusePhase::CheckingNickname)?;
        let mut intent = None;
        let mut started = false;
        let result = bounded(cancel, async {
            let mut payload = self.registration_sms(&operation)?;
            if self.candidate.registration_pending()? {
                return Err(Error::State("registration result is unresolved"));
            }
            if let Some(name) = nickname {
                let name = match model::nickname(name) {
                    Ok(name) => name,
                    Err(error) => {
                        let mut s = operation.lock()?;
                        let a = s.reuse.as_mut().ok_or(Error::StaleResponse)?;
                        a.view.phase = ReusePhase::Nickname;
                        a.view.nickname_error = Some(error);
                        return Ok(());
                    }
                };
                let error =
                    super::super::registration::check(self.candidate.api().as_ref(), name).await;
                if let Some(error) = error {
                    let mut s = operation.lock()?;
                    let a = s.reuse.as_mut().ok_or(Error::StaleResponse)?;
                    a.view.phase = ReusePhase::Nickname;
                    a.view.nickname_error = Some(error);
                    return Ok(());
                }
                payload["nickname"] = name.into();
            } else {
                if !operation
                    .lock()?
                    .reuse
                    .as_ref()
                    .is_some_and(|a| a.view.can_skip_nickname)
                {
                    return Err(Error::InvalidInput("default nickname was not offered"));
                }
                payload["isSkipSetNickname"] = "true".into();
            }
            operation.check()?;
            intent = Some(self.candidate.registration_begin()?);
            started = true;
            operation
                .lock()?
                .reuse
                .as_mut()
                .ok_or(Error::StaleResponse)?
                .registration_started = true;
            self.reuse_phase(&operation, ReusePhase::Registering)?;
            let response = self
                .send(
                    &operation,
                    LoginRequest {
                        path: "/api/w/register/cellphone",
                        payload,
                    },
                    false,
                    Some(LoginStatus::PhoneReuse),
                )
                .await?;
            if !accepted(&response) {
                let rejected = super::super::registration::rejection(&response);
                if new_challenge(&response)
                    || rejected.is_none()
                    || self.candidate.client().registration_authorized()?
                {
                    let mut s = operation.lock()?;
                    s.status = LoginStatus::Error;
                    if let Some(a) = &mut s.reuse {
                        a.view.phase = ReusePhase::RecoveryRequired;
                        a.view.code = response["code"].as_i64();
                    }
                    return Ok(());
                }
                // Complete business rejection is a known outcome. A challenge
                // after force registration is not replayable via phone login.
                self.candidate.registration_not_sent(
                    intent
                        .as_ref()
                        .ok_or(Error::State("missing registration intent"))?,
                )?;
                let mut s = operation.lock()?;
                s.status = LoginStatus::PhoneReuse;
                if response["code"] == 503
                    && let Some(pending) = &mut s.pending
                    && let Some(payload) = pending.payload.as_object_mut()
                {
                    payload.remove("captcha");
                }
                let a = s.reuse.as_mut().ok_or(Error::StaleResponse)?;
                a.view.phase = ReusePhase::Nickname;
                a.view.code = response["code"].as_i64();
                a.view.nickname_error = rejected;
                return Ok(());
            }
            self.reuse_phase(&operation, ReusePhase::RecoveryRequired)?;
            if !self.candidate.client().registration_authorized()? {
                return Err(Error::State(
                    "registration did not return authorization; fresh SMS login required",
                ));
            }
            // Never replay the original SMS request on a 301 here: the force
            // operation may already have unlinked the previous phone binding.
            self.verify_identity(&operation).await
        })
        .await;
        if let Err(error) = &result {
            let not_sent = matches!(error.root(), Error::RequestNotSent(_));
            if not_sent && let Some(intent) = &intent {
                self.candidate.registration_not_sent(intent)?;
            }
            self.reuse_phase(
                &operation,
                if started && !not_sent {
                    ReusePhase::RecoveryRequired
                } else {
                    ReusePhase::Nickname
                },
            )?;
        }
        operation.finish(result)
    }
}
