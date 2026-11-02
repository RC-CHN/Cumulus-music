use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmsDirection {
    Uplink,
    Downlink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmsPhase {
    Loading,
    Ready,
    CodeSent,
    Checking,
    Failed,
    TimedOut,
    RecoveryRequired,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsVerificationView {
    pub id: String,
    pub direction: SmsDirection,
    pub phase: SmsPhase,
    pub code: Option<i64>,
    pub phone_mask: Option<String>,
    pub country_code: Option<String>,
    /// These two service fields must be shown so the user can send the SMS.
    /// They are never accepted back as verification parameters from IPC.
    pub upload_text: Option<String>,
    pub upload_number: Option<String>,
    pub answer_rejected: bool,
}

pub(crate) struct Context {
    pub allowed: bool,
    pub phone_mask: Option<String>,
    pub country_code: Option<String>,
}
pub(crate) fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 256 && !s.chars().any(char::is_control))
        .map(str::to_owned)
}
impl Context {
    pub fn from_list(value: &Value) -> Self {
        let data = &value["data"];
        Self {
            allowed: data["types"]
                .as_array()
                .is_some_and(|types| types.iter().any(|v| v == "UP_SMS")),
            // Official M2992/M2993: authDetail.UP_SMS || phoneMask; ctCode.
            phone_mask: text(&data["authDetail"]["UP_SMS"]).or_else(|| text(&data["phoneMask"])),
            country_code: text(&data["ctCode"])
                .filter(|s| s.len() <= 4 && s.bytes().all(|c| c.is_ascii_digit())),
        }
    }
}

pub(crate) struct Attempt {
    pub view: SmsVerificationView,
    pub refresh_count: u64,
    pub cancel: tokio::sync::watch::Sender<bool>,
}
