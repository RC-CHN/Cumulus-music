//! Bounded native projection of M1368's 8831 penalty orchestration.
//!
//! The recovered PC SDK supplies routes and completion notifications, but the
//! RN account-verify actions behind them are absent. A status read is never a
//! completion notification, permission to replay a password, or authentication.
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{Value, json};

pub const STATUS_PATH: &str = "/api/login/penalty/status";
pub const MAX_STEPS: usize = 32;
pub const MAX_STEP_TEXT_BYTES: usize = 128;
pub const MAX_TICKET_BYTES: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PenaltyStepKind {
    IdentityVerify,
    BindPhone,
    RealName,
    UpgradeWeakPassword,
    UpgradeVersion,
    Unknown,
}
impl PenaltyStepKind {
    fn parse(value: &str) -> Self {
        match value {
            "IDENTITY_VERIFY" => Self::IdentityVerify,
            "BIND_PHONE" => Self::BindPhone,
            "REAL_NAME" => Self::RealName,
            "UPGRADE_WEAK_PASSWORD" => Self::UpgradeWeakPassword,
            "UPGRADE_VERSION" => Self::UpgradeVersion,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PenaltyStepView {
    pub kind: PenaltyStepKind,
    /// Bounded display text only. Never interpret it as a route or endpoint.
    pub type_name: String,
    pub passed: bool,
    /// Official RN resource/version requirement, not the Cumulus app version.
    pub min_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PenaltyPhase {
    Required,
    UpgradeRequired,
    IdentityPending,
    Checking,
    ReadFailed,
    /// The status service reports no pending steps; identity is still unproven.
    StatusPassed,
    Cancelled,
}

/// The action's implementation remains in the unrecovered official RN client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PenaltyBoundary {
    OfficialAccountVerification,
    OfficialIdentityVerification,
    OfficialBindPhone,
    OfficialRealName,
    OfficialPasswordUpgrade,
    OfficialVersionUpgrade,
    UnsupportedStep,
}
impl PenaltyBoundary {
    fn for_step(step: Option<&PenaltyStepView>) -> Self {
        match step.map(|s| s.kind) {
            Some(PenaltyStepKind::IdentityVerify) => Self::OfficialIdentityVerification,
            Some(PenaltyStepKind::BindPhone) => Self::OfficialBindPhone,
            Some(PenaltyStepKind::RealName) => Self::OfficialRealName,
            Some(PenaltyStepKind::UpgradeWeakPassword) => Self::OfficialPasswordUpgrade,
            Some(PenaltyStepKind::UpgradeVersion) => Self::OfficialVersionUpgrade,
            Some(PenaltyStepKind::Unknown) => Self::UnsupportedStep,
            None => Self::OfficialAccountVerification,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PenaltyView {
    pub id: String,
    pub phase: PenaltyPhase,
    pub code: Option<i64>,
    pub steps: Vec<PenaltyStepView>,
    pub current_step: Option<usize>,
    pub boundary: PenaltyBoundary,
    pub can_check_status: bool,
}

/// Fixed read operation. The surrounding JSK adapter adds its normal context;
/// this model never invents a musicTuP header or exports the ticket to the UI.
pub struct PenaltyStatusRequest {
    pub path: &'static str,
    pub method: &'static str,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyStatusOutcome {
    /// SDK's internal replay uses ignorePenalty here. A user status check must
    /// keep this as an external verification boundary and must not replay login.
    IdentityPending,
    NextStep,
    /// This alone must never adopt a candidate or submit stored credentials.
    AllPassed,
}

/// Attempt-local native state. Intentionally neither Debug nor Serialize.
/// It stores no password, phone number, raw response, or authorization route.
pub struct Attempt {
    id: String,
    phase: PenaltyPhase,
    code: Option<i64>,
    steps: Vec<PenaltyStepView>,
    current_step: Option<usize>,
    music_tu_p: Option<String>,
    pub(super) cancel: tokio::sync::watch::Sender<bool>,
}
impl Attempt {
    /// Accept normalized core responses and the original SDK body envelope.
    /// Missing/malformed steps are errors, never evidence of successful checks.
    pub fn from_login_response(response: &Value) -> Result<Self> {
        let response = business_response(response)?;
        if response["code"].as_i64() != Some(8831) {
            return Err(Error::Response("expected penalty login response"));
        }
        let data = response["data"]
            .as_object()
            .ok_or(Error::Response("invalid penalty data"))?;
        let steps = parse_steps(data.get("steps"))?;
        let music_tu_p = match data.get("musicTuP") {
            None | Some(Value::Null) => None,
            Some(Value::String(value))
                if value.len() <= MAX_TICKET_BYTES && !value.chars().any(char::is_control) =>
            {
                (!value.is_empty()).then(|| value.clone())
            }
            _ => return Err(Error::Response("invalid penalty ticket")),
        };
        // Entry M1368:3273 gives pending UPGRADE_VERSION global precedence.
        // Otherwise it opens the RN penalty/identity entry; subsequent RN
        // dispatch is missing, so do not guess a first-pending action here.
        let upgrade = steps
            .iter()
            .position(|s| s.kind == PenaltyStepKind::UpgradeVersion && !s.passed);
        let current_step = upgrade.or_else(|| {
            steps
                .iter()
                .position(|s| s.kind == PenaltyStepKind::IdentityVerify && !s.passed)
        });
        let (cancel, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            phase: if upgrade.is_some() {
                PenaltyPhase::UpgradeRequired
            } else {
                PenaltyPhase::Required
            },
            code: Some(8831),
            steps,
            current_step,
            music_tu_p,
            cancel,
        })
    }

    pub fn view(&self) -> PenaltyView {
        PenaltyView {
            id: self.id.clone(),
            phase: self.phase,
            code: self.code,
            steps: self.steps.clone(),
            current_step: self.current_step,
            boundary: PenaltyBoundary::for_step(
                self.current_step.and_then(|index| self.steps.get(index)),
            ),
            can_check_status: !matches!(
                self.phase,
                PenaltyPhase::Checking | PenaltyPhase::Cancelled
            ) && !*self.cancel.borrow(),
        }
    }

    /// Call only for an explicit user status check, or a native acknowledged
    /// verification continuation. Neither case authorizes an RN write action.
    pub fn begin_status_check(&mut self) -> Result<PenaltyStatusRequest> {
        self.check_active()?;
        if self.phase == PenaltyPhase::Checking {
            return Err(Error::MutationBusy);
        }
        self.phase = PenaltyPhase::Checking;
        self.code = None;
        Ok(PenaltyStatusRequest {
            path: STATUS_PATH,
            method: "POST",
            payload: json!({}),
        })
    }

    /// Only a status-service response may change reported step completion.
    /// There is deliberately no frontend-facing mark-step-completed method.
    pub fn apply_status(&mut self, response: &Value) -> Result<PenaltyStatusOutcome> {
        self.check_active()?;
        if self.phase != PenaltyPhase::Checking {
            return Err(Error::State("penalty status read is not pending"));
        }
        let result = self.read_status(response);
        if result.is_err() {
            self.phase = PenaltyPhase::ReadFailed;
        }
        result
    }

    fn read_status(&mut self, response: &Value) -> Result<PenaltyStatusOutcome> {
        let response = business_response(response)?;
        self.code = response["code"].as_i64();
        if self.code != Some(200) {
            return Err(Error::Response("penalty status request was rejected"));
        }
        let steps = parse_steps(response["data"].get("steps"))?;
        // Status M1368:5288 checks the first IDENTITY_VERIFY before selecting
        // any other pending step. This differs from entry upgrade precedence.
        let identity = steps
            .iter()
            .position(|s| s.kind == PenaltyStepKind::IdentityVerify);
        let identity_pending = identity.is_some_and(|index| !steps[index].passed);
        let current_step = if identity_pending {
            identity
        } else {
            steps.iter().position(|s| !s.passed)
        };
        self.steps = steps;
        self.current_step = current_step;
        let outcome = if identity_pending {
            self.phase = PenaltyPhase::IdentityPending;
            PenaltyStatusOutcome::IdentityPending
        } else if current_step.is_some() {
            self.phase = PenaltyPhase::Required;
            PenaltyStatusOutcome::NextStep
        } else {
            self.phase = PenaltyPhase::StatusPassed;
            PenaltyStatusOutcome::AllPassed
        };
        Ok(outcome)
    }

    /// A failed/unknown read retains the last validated steps and is retryable.
    pub fn status_failed(&mut self) {
        if self.phase == PenaltyPhase::Checking && !*self.cancel.borrow() {
            self.phase = PenaltyPhase::ReadFailed;
        }
    }

    pub fn cancel(&mut self) {
        self.cancel.send_replace(true);
        self.music_tu_p = None;
        self.current_step = None;
        self.phase = PenaltyPhase::Cancelled;
    }

    fn check_active(&self) -> Result<()> {
        if *self.cancel.borrow() || self.phase == PenaltyPhase::Cancelled {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

fn business_response(response: &Value) -> Result<&Value> {
    let body = response.get("body").unwrap_or(response);
    if !body.is_object() {
        return Err(Error::Response("invalid penalty response"));
    }
    Ok(body)
}

fn bounded_text(value: &Value) -> Result<String> {
    value
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= MAX_STEP_TEXT_BYTES
                && !s.chars().any(char::is_control)
                && !s.contains("://")
        })
        .map(str::to_owned)
        .ok_or(Error::Response("invalid penalty step text"))
}

fn min_version(value: Option<&Value>) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(Value::String(_)) => value.map(bounded_text).transpose(),
        Some(Value::Number(value)) if value.as_f64() == Some(0.0) => Ok(None),
        Some(Value::Number(value)) => Ok(Some(value.to_string())),
        _ => Err(Error::Response("invalid penalty version requirement")),
    }
}

fn parse_steps(value: Option<&Value>) -> Result<Vec<PenaltyStepView>> {
    let items = value
        .and_then(Value::as_array)
        .filter(|items| items.len() <= MAX_STEPS)
        .ok_or(Error::Response("invalid penalty steps"))?;
    items
        .iter()
        .map(|item| {
            let fields = item
                .as_object()
                .ok_or(Error::Response("invalid penalty step"))?;
            let type_name = bounded_text(
                fields
                    .get("type")
                    .ok_or(Error::Response("missing penalty step type"))?,
            )?;
            let state = bounded_text(
                fields
                    .get("state")
                    .ok_or(Error::Response("missing penalty step state"))?,
            )?;
            Ok(PenaltyStepView {
                kind: PenaltyStepKind::parse(&type_name),
                type_name,
                passed: state == "PASSED",
                min_version: min_version(fields.get("minVersion"))?,
            })
        })
        .collect()
}
