use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReusePhase {
    Loading,
    Choice,
    LoginConsent,
    RegisterConsent,
    Nickname,
    ReadFailed,
    LoginFailed,
    Confirming,
    Hinting,
    CheckingNickname,
    Registering,
    RecoveryRequired,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReuseChoice {
    Login,
    Register,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationBlocked {
    FreshSmsRequired,
    UnconfirmedRegistration,
}
use super::super::nickname::NicknameError;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReuseVerificationView {
    pub id: String,
    pub phase: ReusePhase,
    pub code: Option<i64>,
    pub masked_nickname: Option<String>,
    pub created_at: Option<Value>,
    pub registration_blocked: Option<RegistrationBlocked>,
    pub can_skip_nickname: bool,
    pub suggested_nickname: Option<String>,
    pub nickname_error: Option<NicknameError>,
}
pub(super) fn masked(value: &Value) -> Option<String> {
    let text = value.as_str().filter(|s| {
        !s.is_empty() && s.chars().count() <= 512 && !s.chars().any(char::is_control)
    })?;
    // Official AccountCheck uses charAt(0), i.e. one UTF-16 code unit.
    Some(format!(
        "{}****",
        String::from_utf16_lossy(&[text.encode_utf16().next()?])
    ))
}
pub(super) fn created(value: &Value) -> Option<Value> {
    if value
        .as_i64()
        .is_some_and(|n| (0..=253_402_300_799_999).contains(&n))
    {
        return Some(value.clone());
    }
    value
        .as_str()
        .filter(|s| {
            (10..=35).contains(&s.len())
                && s.as_bytes()[4] == b'-'
                && s.as_bytes()[7] == b'-'
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || b"-TtZz: .+".contains(&b))
        })
        .map(|_| value.clone())
}
pub(super) fn nickname(value: &str) -> std::result::Result<&str, NicknameError> {
    super::super::nickname::validate(value)
}
