use crate::account::login::{
    DeviceVerificationKind, DeviceVerificationView, EmailVerificationView, LoginStatus,
    PhoneEntryView, QuestionVerificationView, ReuseChoice, ReuseVerificationView, SmsDirection,
    SmsVerificationView,
};
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
    PhoneStart {
        phone: String,
        country_code: String,
    },
    PhoneRetry {
        entry_id: String,
    },
    PhoneResend {
        entry_id: String,
    },
    PhoneCode {
        entry_id: String,
        code: String,
        remember: bool,
    },
    PhoneNicknameHint {
        entry_id: String,
    },
    PhoneRegister {
        entry_id: String,
        nickname: Option<String>,
    },
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
    SmsStart {
        direction: SmsDirection,
    },
    SmsRefresh {
        verification_id: String,
    },
    SmsCheck {
        verification_id: String,
    },
    SmsStop {
        verification_id: String,
    },
    DeviceStart {
        method: DeviceVerificationKind,
    },
    DevicePoll {
        verification_id: String,
    },
    DeviceVerify {
        verification_id: String,
        code: String,
    },
    DeviceCancel {
        verification_id: String,
    },
    DeviceStop {
        verification_id: String,
    },
    SecurityMailSend,
    SecurityMailResend {
        verification_id: String,
    },
    SecurityMailStop {
        verification_id: String,
    },
    SecurityMailVerify {
        verification_id: String,
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
        question_id: String,
        answer: String,
    },
    ReuseRead,
    ReuseBack {
        verification_id: String,
    },
    ReuseChoose {
        verification_id: String,
        choice: ReuseChoice,
    },
    ReuseConfirm {
        verification_id: String,
    },
    ReusePrepareRegister {
        verification_id: String,
    },
    ReuseNicknameHint {
        verification_id: String,
    },
    ReuseRegister {
        verification_id: String,
        nickname: Option<String>,
    },
    Reconcile,
    PenaltyCheck {
        verification_id: String,
    },
    SecurityQrPoll {
        verification_id: String,
    },
    SecurityQrRefresh {
        verification_id: String,
    },
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginChallengeView {
    pub methods: Vec<String>,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub phone_mask: Option<String>,
    pub country_code: Option<String>,
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
    pub tag: Option<String>,
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
    pub notice: Option<crate::account::login::LoginNoticeView>,
    pub penalty: Option<crate::account::login::penalty::PenaltyView>,
    pub security_qr: Option<crate::account::login::SecurityQrView>,
    pub challenge: LoginChallengeView,
    pub device: Option<DeviceVerificationView>,
    pub sms: Option<SmsVerificationView>,
    pub email: Option<EmailVerificationView>,
    pub question: Option<QuestionVerificationView>,
    pub reuse: Option<ReuseVerificationView>,
    pub phone_entry: Option<PhoneEntryView>,
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
