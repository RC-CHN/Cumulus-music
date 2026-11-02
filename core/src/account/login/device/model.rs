use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceVerificationKind {
    Confirm,
    Scan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceVerificationPhase {
    Starting,
    Waiting,
    Scanned,
    CodeRequired,
    Completed,
    Expired,
    Rejected,
    Blocked,
    Error,
    RecoveryRequired,
    Cancelled,
    StopUnconfirmed,
    Stopped,
}
impl DeviceVerificationPhase {
    pub(super) fn active(self) -> bool {
        matches!(
            self,
            Self::Waiting
                | Self::Scanned
                | Self::CodeRequired
                | Self::Error
                | Self::RecoveryRequired
        )
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceVerificationView {
    pub id: String,
    pub kind: DeviceVerificationKind,
    pub phase: DeviceVerificationPhase,
    pub code: Option<i64>,
    pub device_name: Option<String>,
    pub safe_device_count: Option<u64>,
    pub poll_after_ms: u64,
    pub modules: Option<Vec<Vec<bool>>>,
    pub answer_rejected: bool,
}

// A private service polling token is never a frontend operation identifier.
pub(crate) struct Attempt {
    pub view: DeviceVerificationView,
    pub token: Option<String>,
    pub uncertain_submit: bool,
}
impl DeviceVerificationKind {
    pub(super) fn path(self, action: &str) -> &'static str {
        match (self, action) {
            (Self::Confirm, "start") => "/api/login/origin-device/apply/start",
            (Self::Confirm, "poll") => "/api/login/origin-device/apply/check-status",
            (Self::Confirm, "verify") => "/api/login/origin-device/apply/check",
            (Self::Confirm, _) => "/api/login/origin-device/apply/cancel",
            (Self::Scan, "start") => "/api/login/origin-device/scan-apply/start",
            (Self::Scan, "poll") => "/api/login/origin-device/scan-apply/check",
            (Self::Scan, _) => "/api/login/origin-device/scan-apply/cancel",
        }
    }
}
