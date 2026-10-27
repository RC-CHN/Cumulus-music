use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStatus {
    Ready,
    Submitting,
    SendingSms,
    CodeSent,
    LoadingVerification,
    Verifying,
    CheckingIdentity,
    Authorized,
    Authenticated,
    Verification,
    Captcha,
    TwoFactor,
    PhoneReuse,
    SecurityQuestion,
    SecurityQr,
    UpgradeRequired,
    RiskNotice,
    RateLimited,
    AccountDisabled,
    Rejected,
    Cancelled,
    Error,
}

impl LoginStatus {
    pub(super) fn verification(self) -> bool {
        matches!(
            self,
            Self::Verification | Self::TwoFactor | Self::PhoneReuse | Self::SecurityQuestion
        )
    }
}

pub fn classify(response: &Value) -> LoginStatus {
    use LoginStatus::*;
    match response["code"].as_i64() {
        Some(200) => Authorized,
        Some(702 | 8840) => Verification,
        Some(8820) => UpgradeRequired,
        Some(8821) => Captcha,
        Some(8830) => TwoFactor,
        Some(8860) => PhoneReuse,
        Some(8861) => SecurityQuestion,
        Some(509) => RateLimited,
        Some(304) => AccountDisabled,
        Some(8810 | 10004) => match response["toast"]["type"].as_i64() {
            Some(1) => SecurityQr,
            Some(2) => UpgradeRequired,
            Some(3) => RiskNotice,
            _ => Rejected,
        },
        _ => Rejected,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationAction {
    MailSend,
    MailVerify,
    SecurityMailSend,
    SecurityMailVerify,
    SmsSend,
    SmsVerify,
    OrderList,
    OrderVerify,
    BindPhoneVerify,
    QuestionGet,
    QuestionVerify,
    PhoneReuseUser,
    PhoneReuseConfirm,
}

pub(super) struct VerificationRequest {
    pub path: &'static str,
    pub payload: Value,
    pub sends_message: bool,
    pub completes: bool,
}

impl VerificationAction {
    pub(super) fn request(self, mut params: Value) -> Result<VerificationRequest> {
        use VerificationAction::*;
        let (path, fields, completes): (&str, &[&str], bool) = match self {
            MailSend => ("/api/login/2fa/mail/captcha/send", &["scene"], false),
            MailVerify => (
                "/api/login/2fa/mail/captcha/check",
                &["scene", "captcha"],
                true,
            ),
            SecurityMailSend => ("/api/user/2fa/securityEmail/send", &["scene"], false),
            SecurityMailVerify => (
                "/api/user/2fa/securityEmail/verify",
                &["scene", "captcha"],
                true,
            ),
            SmsSend => ("/api/middle/account/captcha/sent", &["scene"], false),
            SmsVerify => ("/api/middle/account/captcha/verify", &["captcha"], true),
            OrderList => ("/api/user/2fa/order/list", &["scene"], false),
            OrderVerify => (
                "/api/user/2fa/order/check",
                &["scene", "paymentMethod", "paymentOrderId"],
                true,
            ),
            BindPhoneVerify => (
                "/api/user/2fa/bindcellphone",
                &["phone", "countrycode", "captcha"],
                true,
            ),
            QuestionGet => ("/api/login/cellphone/twice/question/get", &["scene"], false),
            QuestionVerify => (
                "/api/login/cellphone/twice/question/verify",
                &["scene", "ans"],
                true,
            ),
            PhoneReuseUser => ("/api/login/cellphone/twice/used/user/get", &[], false),
            PhoneReuseConfirm => ("/api/login/cellphone/twice/used/confirm", &[], true),
        };
        let map = params.as_object_mut().ok_or(Error::InvalidInput(
            "expected verification parameter object",
        ))?;
        if map.keys().any(|k| !fields.contains(&k.as_str()))
            || fields
                .iter()
                .any(|k| *k != "scene" && !map.contains_key(*k))
        {
            return Err(Error::InvalidInput("invalid verification fields"));
        }
        if fields.contains(&"scene") {
            map.entry("scene").or_insert_with(|| "0".into());
        }
        if let Some(order) = map.get_mut("paymentOrderId") {
            *order = order
                .as_str()
                .ok_or(Error::InvalidInput("expected payment order text"))?
                .trim()
                .into();
        }
        Ok(VerificationRequest {
            path,
            payload: params,
            sends_message: matches!(self, MailSend | SecurityMailSend | SmsSend),
            completes,
        })
    }
}
