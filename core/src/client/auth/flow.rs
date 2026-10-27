use super::*;
use crate::account::login::{PhoneCredential, PhoneLogin, VerificationAction};
use serde_json::{Value, json};

impl MusicClient {
    pub async fn begin_login(
        &self,
        epoch: &str,
        mode: LoginMode,
        saved_id: Option<&str>,
    ) -> Result<LoginView> {
        let _gate = self.together.gate.lock().await;
        self.auth_token(epoch)?;
        self.check_account_transition()?;
        if (mode == LoginMode::SavedAccount) != saved_id.is_some() {
            return Err(Error::InvalidInput("invalid saved account login mode"));
        }
        let flow = Arc::new(CredentialLogin::new(
            self.session.clone(),
            self.login_candidate(saved_id)?,
            self.auth.cooldown.clone(),
            self.auth.guardian.clone(),
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
        let initial = matches!(state.status, LoginStatus::Ready | LoginStatus::CodeSent);
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
        match command {
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
                    next.phone_mask = view::short_text(&r["data"]["phoneMask"]);
                    projection = Some(next);
                }
            }
            LoginCommand::SecurityMailSend => {
                offered("SECURITY_EMAIL")?;
                flow.verification(VerificationAction::SecurityMailSend, json!({"scene":"0"}))
                    .await?;
            }
            LoginCommand::SecurityMailVerify { code } => {
                offered("SECURITY_EMAIL")?;
                flow.verification(
                    VerificationAction::SecurityMailVerify,
                    json!({"scene":"0","captcha":code}),
                )
                .await?;
            }
            LoginCommand::VerificationSmsSend => {
                offered("UP_SMS")?;
                flow.verification(VerificationAction::SmsSend, json!({"scene":"0"}))
                    .await?;
            }
            LoginCommand::VerificationSmsVerify { code } => {
                offered("UP_SMS")?;
                flow.verification(VerificationAction::SmsVerify, json!({"captcha":code}))
                    .await?;
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
                if state.status != LoginStatus::SecurityQuestion {
                    return Err(Error::State("no security question pending"));
                }
                let r = flow
                    .verification(VerificationAction::QuestionGet, json!({"scene":"0"}))
                    .await?;
                if r["code"] == 200 {
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
                            })
                        })
                        .collect();
                    next.answers.push(LoginAnswerView {
                        value: "none".into(),
                        title: "以上都不是".into(),
                        subtitle: None,
                        picture: None,
                    });
                    projection = Some(next);
                }
            }
            LoginCommand::QuestionAnswer { answer } => {
                if state.status != LoginStatus::SecurityQuestion
                    || !view.answers.iter().any(|v| v.value == answer)
                {
                    return Err(Error::InvalidInput("answer was not offered"));
                }
                flow.verification(
                    VerificationAction::QuestionVerify,
                    json!({"scene":"0","ans":answer}),
                )
                .await?;
            }
            LoginCommand::ReuseRead => {
                if state.status != LoginStatus::PhoneReuse {
                    return Err(Error::State("no phone reuse confirmation pending"));
                }
                let r = flow
                    .verification(VerificationAction::PhoneReuseUser, json!({}))
                    .await?;
                if r["code"] == 200 {
                    let mut next = view.clone();
                    next.nickname = view::short_text(&r["data"]["nickName"])
                        .and_then(|s| s.chars().next().map(|c| format!("{c}****")));
                    next.avatar_url = mapping::artwork(
                        r["data"]
                            .get("avatarImgUrl")
                            .unwrap_or(&r["data"]["avatarUrl"]),
                    );
                    next.reuse_confirmation = true;
                    projection = Some(next);
                }
            }
            LoginCommand::ReuseConfirm => {
                if state.status != LoginStatus::PhoneReuse || !view.reuse_confirmation {
                    return Err(Error::State("read the previous account before confirming"));
                }
                flow.verification(VerificationAction::PhoneReuseConfirm, json!({}))
                    .await?;
            }
            LoginCommand::Reconcile => {
                flow.reconcile().await?;
            }
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
