use super::super::NicknameError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneEntryPhase {
    CheckingAccount,
    CheckFailed,
    SendingCode,
    CodeRequired,
    Verifying,
    Nickname,
    Hinting,
    CheckingNickname,
    Registering,
    RecoveryRequired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneEntryIssue {
    CheckFailed,
    SendFailed,
    SendUnconfirmed,
    Cooldown,
    DailyLimit,
    FivePerDay,
    CodeInvalid,
    VerificationUnconfirmed,
    LoginRejected,
    LoginUnconfirmed,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneEntryView {
    pub id: String,
    pub phase: PhoneEntryPhase,
    pub code: Option<i64>,
    pub new_user: bool,
    pub can_skip_nickname: bool,
    pub suggested_nickname: Option<String>,
    pub nickname_error: Option<NicknameError>,
    pub issue: Option<PhoneEntryIssue>,
    pub registration_blocked: bool,
    pub can_reconcile: bool,
}
