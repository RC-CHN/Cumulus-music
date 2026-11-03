use super::*;
use crate::account::login::{
    DeviceVerificationKind, DeviceVerificationPhase, PhoneCredential, PhoneLogin,
    VerificationAction,
};
use serde_json::{Value, json};

impl MusicClient {
    pub async fn begin_login(
        &self,
        epoch: &str,
        mode: LoginMode,
        saved_id: Option<&str>,
    ) -> Result<LoginView> {
        let _gate = self.together.gate.lock().await;
        let token = self.auth_token(epoch)?;
        self.check_account_transition()?;
        if (mode == LoginMode::SavedAccount) != saved_id.is_some() {
            return Err(Error::InvalidInput("invalid saved account login mode"));
        }
        let protocol = if mode == LoginMode::SavedAccount {
            crate::account::login::LoginProtocol::Legacy
        } else {
            self.session.new_login_protocol(&token).await?
        };
        let flow = Arc::new(CredentialLogin::new_with_protocol(
            self.session.clone(),
            self.login_candidate(saved_id)?,
            self.auth.cooldown.clone(),
            self.auth.guardian.clone(),
            protocol,
        )?);
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.auth.replace(PendingLogin {
            id: id.clone(),
            flow: PendingFlow::Credentials { mode, flow },
            view: LoginChallengeView::default(),
            captcha: Default::default(),
        })?;
        self.login_snapshot(&id)
    }

    pub async fn login_command(&self, id: &str, command: LoginCommand) -> Result<LoginView> {
        if let LoginCommand::SecurityMailStop { verification_id } = &command {
            let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            let pending = slot
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or(Error::StaleResponse)?;
            pending.captcha.check_idle()?;
            let PendingFlow::Credentials { flow, .. } = &pending.flow else {
                return Err(Error::StaleResponse);
            };
            flow.email_stop(verification_id)?;
            drop(slot);
            return self.login_snapshot(id);
        }
        if let LoginCommand::SmsStop { verification_id } = &command {
            let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            let pending = slot
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or(Error::StaleResponse)?;
            pending.captcha.check_idle()?;
            let PendingFlow::Credentials { flow, .. } = &pending.flow else {
                return Err(Error::StaleResponse);
            };
            flow.sms_stop(verification_id)?;
            drop(slot);
            return self.login_snapshot(id);
        }
        if let LoginCommand::DeviceStop { verification_id } = &command {
            let flow = {
                let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
                let pending = slot
                    .as_ref()
                    .filter(|p| p.id == id)
                    .ok_or(Error::StaleResponse)?;
                pending.captcha.check_idle()?;
                let PendingFlow::Credentials { flow, .. } = &pending.flow else {
                    return Err(Error::StaleResponse);
                };
                flow.clone()
            };
            flow.device_leave(verification_id).await?;
            return self.login_snapshot(id);
        }
        let _gate = self.together.gate.lock().await;
        self.check_account_transition()?;
        let (mode, flow, view) = {
            let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            let p = slot
                .as_ref()
                .filter(|p| p.id == id)
                .ok_or(Error::StaleResponse)?;
            let PendingFlow::Credentials { mode, flow } = &p.flow else {
                return Err(Error::StaleResponse);
            };
            p.captcha.check_idle()?;
            (*mode, flow.clone(), p.view.clone())
        };
        let state = flow.snapshot()?;
        if flow.penalty_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::PenaltyCheck { .. } | LoginCommand::Reconcile
            )
        {
            return Err(Error::State(
                "finish or cancel the current account verification",
            ));
        }
        if flow.security_qr_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::SecurityQrPoll { .. }
                    | LoginCommand::SecurityQrRefresh { .. }
                    | LoginCommand::Reconcile
            )
        {
            return Err(Error::State("finish or cancel the current security QR"));
        }
        let device = flow.device_snapshot()?;
        if device.as_ref().is_some_and(|v| {
            matches!(
                v.phase,
                DeviceVerificationPhase::Starting
                    | DeviceVerificationPhase::Waiting
                    | DeviceVerificationPhase::Scanned
                    | DeviceVerificationPhase::CodeRequired
                    | DeviceVerificationPhase::Error
                    | DeviceVerificationPhase::RecoveryRequired
            )
        }) && !matches!(
            &command,
            LoginCommand::DevicePoll { .. }
                | LoginCommand::DeviceVerify { .. }
                | LoginCommand::DeviceCancel { .. }
                | LoginCommand::Reconcile
        ) {
            return Err(Error::State(
                "stop device verification before changing methods",
            ));
        }
        let initial = matches!(state.status, LoginStatus::Ready | LoginStatus::CodeSent);
        if flow.phone_entry_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::PhoneRetry { .. }
                    | LoginCommand::PhoneResend { .. }
                    | LoginCommand::PhoneCode { .. }
                    | LoginCommand::PhoneNicknameHint { .. }
                    | LoginCommand::PhoneRegister { .. }
                    | LoginCommand::Reconcile
            )
        {
            return Err(Error::State(
                "finish or cancel the current phone entry before changing credentials",
            ));
        }
        if flow.reuse_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::ReuseRead
                    | LoginCommand::ReuseBack { .. }
                    | LoginCommand::ReuseChoose { .. }
                    | LoginCommand::ReuseConfirm { .. }
                    | LoginCommand::ReusePrepareRegister { .. }
                    | LoginCommand::ReuseNicknameHint { .. }
                    | LoginCommand::ReuseRegister { .. }
                    | LoginCommand::Reconcile
            )
        {
            return Err(Error::State(
                "finish or cancel phone reuse before changing methods",
            ));
        }
        if flow.question_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::QuestionRead
                    | LoginCommand::QuestionAnswer { .. }
                    | LoginCommand::Reconcile
            )
        {
            return Err(Error::State(
                "finish or cancel account question before changing methods",
            ));
        }
        if flow.email_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::SecurityMailVerify { .. }
                    | LoginCommand::SecurityMailResend { .. }
                    | LoginCommand::Reconcile
            )
        {
            return Err(Error::State(
                "stop email verification before changing methods",
            ));
        }
        if flow.sms_snapshot()?.is_some()
            && !matches!(
                &command,
                LoginCommand::SmsCheck { .. }
                    | LoginCommand::SmsRefresh { .. }
                    | LoginCommand::VerificationSmsSend
                    | LoginCommand::VerificationSmsVerify { .. }
                    | LoginCommand::Reconcile
            )
        {
            return Err(Error::State(
                "stop SMS verification before changing methods",
            ));
        }
        let initial_mode = |expected| -> Result<()> {
            if mode != expected || !initial {
                return Err(Error::State("start a new login attempt"));
            }
            Ok(())
        };
        let offered = |method: &str| -> Result<()> {
            if state.status != LoginStatus::TwoFactor || !view.methods.iter().any(|s| s == method) {
                return Err(Error::State("verification method was not offered"));
            }
            Ok(())
        };
        let mut projection = None;
        let mut completed_device = false;
        match command {
            LoginCommand::PenaltyCheck { verification_id } => {
                flow.penalty_check(&verification_id).await?;
            }
            LoginCommand::SecurityQrPoll { verification_id } => {
                flow.security_qr_poll(&verification_id, None).await?;
            }
            LoginCommand::SecurityQrRefresh { verification_id } => {
                flow.security_qr_refresh(&verification_id).await?;
            }
            LoginCommand::PhoneStart {
                phone,
                country_code,
            } => {
                initial_mode(LoginMode::PhoneSms)?;
                flow.phone_entry_start(&phone, &country_code).await?;
            }
            LoginCommand::PhoneRetry { entry_id } => {
                flow.phone_entry_retry(&entry_id).await?;
            }
            LoginCommand::PhoneResend { entry_id } => {
                flow.phone_entry_resend(&entry_id).await?;
            }
            LoginCommand::PhoneCode {
                entry_id,
                code,
                remember,
            } => {
                flow.phone_entry_submit(&entry_id, &code, remember).await?;
            }
            LoginCommand::PhoneNicknameHint { entry_id } => {
                flow.phone_entry_nickname_hint(&entry_id).await?;
            }
            LoginCommand::PhoneRegister { entry_id, nickname } => {
                flow.phone_entry_register(&entry_id, nickname.as_deref())
                    .await?;
            }
            LoginCommand::PhonePassword {
                phone,
                country_code,
                password,
                remember,
            } => {
                initial_mode(LoginMode::PhonePassword)?;
                flow.phone(PhoneLogin {
                    phone: &phone,
                    country_code: &country_code,
                    credential: PhoneCredential::Password(&password),
                    remember,
                    secure_captcha: None,
                })
                .await?;
            }
            LoginCommand::PhoneSms {
                phone,
                country_code,
                code,
                remember,
            } => {
                initial_mode(LoginMode::PhoneSms)?;
                flow.phone(PhoneLogin {
                    phone: &phone,
                    country_code: &country_code,
                    credential: PhoneCredential::Sms(&code),
                    remember,
                    secure_captcha: None,
                })
                .await?;
            }
            LoginCommand::Email {
                address,
                password,
                remember,
            } => {
                initial_mode(LoginMode::Email)?;
                flow.email(&address, &password, remember, None).await?;
            }
            LoginCommand::SendSms {
                phone,
                country_code,
            } => {
                initial_mode(LoginMode::PhoneSms)?;
                flow.send_sms(&phone, &country_code, false).await?;
            }
            LoginCommand::VerificationMethods => {
                let r = flow.verification_methods().await?;
                if r["code"] == 200 {
                    let mut next = view.clone();
                    next.methods = r["data"]["types"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .filter(|s| {
                            matches!(
                                *s,
                                "DEVICE_CONFIRM"
                                    | "DEVICE_SCAN"
                                    | "WEICHAT_AUTH"
                                    | "UP_SMS"
                                    | "BINDING_CELLPHONE"
                                    | "SECURITY_EMAIL"
                            )
                        })
                        .map(str::to_owned)
                        .collect();
                    next.nickname = view::short_text(&r["data"]["nickName"]);
                    next.avatar_url = mapping::artwork(&r["data"]["avatarUrl"]);
                    next.phone_mask = view::short_text(&r["data"]["authDetail"]["UP_SMS"])
                        .filter(|s| !s.is_empty())
                        .or_else(|| view::short_text(&r["data"]["phoneMask"]));
                    next.country_code = view::short_text(&r["data"]["ctCode"]).filter(|s| {
                        !s.is_empty() && s.len() <= 4 && s.bytes().all(|c| c.is_ascii_digit())
                    });
                    projection = Some(next);
                }
            }
            LoginCommand::SmsStart { direction } => {
                offered("UP_SMS")?;
                flow.sms_start(direction).await?;
            }
            LoginCommand::SmsRefresh { verification_id } => {
                flow.sms_refresh(&verification_id).await?;
            }
            LoginCommand::SmsCheck { verification_id } => {
                completed_device = flow.sms_check_up(&verification_id).await?;
            }
            LoginCommand::SmsStop { .. } => {
                unreachable!("local stop handled before the business gate")
            }
            LoginCommand::DeviceStart { method } => {
                offered(if method == DeviceVerificationKind::Confirm {
                    "DEVICE_CONFIRM"
                } else {
                    "DEVICE_SCAN"
                })?;
                flow.device_start(method, self.auth.device_name.as_deref().unwrap_or("PC"))
                    .await?;
            }
            LoginCommand::DevicePoll { verification_id } => {
                completed_device = flow.device_poll(&verification_id).await?;
            }
            LoginCommand::DeviceVerify {
                verification_id,
                code,
            } => {
                completed_device = flow.device_verify(&verification_id, &code).await?;
            }
            LoginCommand::DeviceCancel { verification_id } => {
                flow.device_cancel(&verification_id).await?;
            }
            LoginCommand::DeviceStop { .. } => {
                unreachable!("local stop handled before the business gate")
            }
            LoginCommand::SecurityMailSend => {
                offered("SECURITY_EMAIL")?;
                flow.email_start().await?;
            }
            LoginCommand::SecurityMailResend { verification_id } => {
                flow.email_resend(&verification_id).await?;
            }
            LoginCommand::SecurityMailStop { .. } => {
                unreachable!("local email stop handled before the business gate")
            }
            LoginCommand::SecurityMailVerify {
                verification_id,
                code,
            } => {
                completed_device = flow.email_verify(&verification_id, &code).await?;
            }
            LoginCommand::VerificationSmsSend => {
                flow.sms_send_down().await?;
            }
            LoginCommand::VerificationSmsVerify { code } => {
                completed_device = flow.sms_verify_down(&code).await?;
            }
            LoginCommand::BindPhoneVerify {
                phone,
                country_code,
                code,
            } => {
                offered("BINDING_CELLPHONE")?;
                flow.verification(
                    VerificationAction::BindPhoneVerify,
                    json!({"phone":phone,"countrycode":country_code,"captcha":code}),
                )
                .await?;
            }
            LoginCommand::QuestionRead => {
                let r = flow.question_read().await?;
                if flow
                    .question_snapshot()?
                    .is_some_and(|q| q.phase == crate::account::login::QuestionPhase::Ready)
                {
                    let mut next = view.clone();
                    next.question = view::short_text(&r["data"]["title"]);
                    next.answers = r["data"]["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| {
                            view::short_text(&v["title"]).map(|title| LoginAnswerView {
                                value: title.clone(),
                                title,
                                subtitle: view::short_text(&v["subtitle"]),
                                picture: mapping::artwork(&v["pic"]),
                                tag: mapping::artwork(&v["tag"]),
                            })
                        })
                        .collect();
                    next.answers.push(LoginAnswerView {
                        value: "none".into(),
                        title: "以上都不是".into(),
                        subtitle: None,
                        picture: None,
                        tag: None,
                    });
                    projection = Some(next);
                }
                completed_device = flow.question_snapshot()?.is_none();
            }
            LoginCommand::QuestionAnswer {
                question_id,
                answer,
            } => {
                flow.question_answer(&question_id, &answer).await?;
                completed_device = flow.question_snapshot()?.is_none();
            }
            LoginCommand::ReuseRead => {
                let r = flow.reuse_read().await?;
                projection = Some(reuse_projection(&flow, &r));
                completed_device = flow.reuse_snapshot()?.is_none();
            }
            LoginCommand::ReuseBack { verification_id } => {
                let r = flow.reuse_back(&verification_id).await?;
                projection = Some(reuse_projection(&flow, &r));
                completed_device = flow.reuse_snapshot()?.is_none();
            }
            LoginCommand::ReuseChoose {
                verification_id,
                choice,
            } => {
                flow.reuse_choose(&verification_id, choice)?;
            }
            LoginCommand::ReuseConfirm { verification_id } => {
                flow.reuse_confirm(&verification_id).await?;
                completed_device = flow.reuse_snapshot()?.is_none();
            }
            LoginCommand::ReusePrepareRegister { verification_id } => {
                flow.reuse_prepare_register(&verification_id).await?;
            }
            LoginCommand::ReuseNicknameHint { verification_id } => {
                flow.reuse_nickname_hint(&verification_id).await?;
            }
            LoginCommand::ReuseRegister {
                verification_id,
                nickname,
            } => {
                flow.reuse_register(&verification_id, nickname.as_deref())
                    .await?;
            }
            LoginCommand::Reconcile => {
                if let Some(view) = flow.penalty_snapshot()? {
                    flow.penalty_check(&view.id).await?;
                } else if let Some(view) = flow.security_qr_snapshot()? {
                    flow.security_qr_reconcile(&view.id).await?;
                } else if flow.phone_entry_snapshot()?.is_some() {
                    flow.phone_entry_reconcile().await?;
                } else if flow.reuse_snapshot()?.is_some() {
                    flow.reuse_reconcile().await?;
                } else if flow.question_snapshot()?.is_some() {
                    flow.question_reconcile().await?;
                } else {
                    flow.reconcile().await?;
                }
            }
        }
        if completed_device && flow.snapshot()?.status != LoginStatus::Authenticated {
            projection = Some(LoginChallengeView::default());
        }
        if let Some(view) = projection {
            let mut slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            let pending = slot
                .as_mut()
                .filter(|p| p.id == id)
                .ok_or(Error::StaleResponse)?;
            pending.view = view;
        }
        let result = self.login_snapshot(id)?;
        if result.status == LoginStatus::Authenticated {
            self.suspend_account_media().await?;
        }
        Ok(result)
    }
}

fn reuse_projection(flow: &CredentialLogin, r: &Value) -> LoginChallengeView {
    let mut next = LoginChallengeView::default();
    if let Ok(Some(a)) = flow.reuse_snapshot()
        && a.phase == crate::account::login::ReusePhase::Choice
    {
        next.nickname = a.masked_nickname;
        next.avatar_url = mapping::artwork(
            r["data"]
                .get("avatarImgUrl")
                .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                .unwrap_or(&r["data"]["avatarUrl"]),
        );
        next.reuse_confirmation = true;
    }
    next
}
