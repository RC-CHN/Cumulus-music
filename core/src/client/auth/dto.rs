use crate::account::login::LoginStatus;
use serde::{Deserialize, Serialize};

/// Only the bounded resulting state crosses IPC; never the CAPTCHA proof.
#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LoginCaptchaView {
    Credentials(Box<LoginView>),
    Qr(crate::account::QrStatus),
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMode {
    PhonePassword,
    PhoneSms,
    Email,
    SavedAccount,
}

// Intentionally no Debug: these request fields can contain passwords or OTPs.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoginCommand {
    PhonePassword {
        phone: String,
        country_code: String,
        password: String,
        remember: bool,
    },
    PhoneSms {
        phone: String,
        country_code: String,
        code: String,
        remember: bool,
    },
    Email {
        address: String,
        password: String,
        remember: bool,
    },
    SendSms {
        phone: String,
        country_code: String,
    },
    VerificationMethods,
    SecurityMailSend,
    SecurityMailVerify {
        code: String,
    },
    VerificationSmsSend,
    VerificationSmsVerify {
        code: String,
    },
    BindPhoneVerify {
        phone: String,
        country_code: String,
        code: String,
    },
    QuestionRead,
    QuestionAnswer {
        answer: String,
    },
    ReuseRead,
    ReuseConfirm,
    Reconcile,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginChallengeView {
    pub methods: Vec<String>,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub phone_mask: Option<String>,
    pub question: Option<String>,
    pub answers: Vec<LoginAnswerView>,
    pub reuse_confirmation: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAnswerView {
    pub value: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub picture: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoutView {
    pub account: super::super::dto::AccountView,
    pub remote_revoked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginView {
    pub operation_id: String,
    pub mode: LoginMode,
    pub status: LoginStatus,
    pub code: Option<i64>,
    pub busy: bool,
    pub captcha_available: bool,
    pub retry_after_ms: u64,
    pub interrupted: bool,
    pub challenge: LoginChallengeView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccountView {
    pub id: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub current: bool,
    pub auto_login: bool,
    pub removal_pending: bool,
}
