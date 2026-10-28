use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Liked,
    Albums,
    Artists,
    Mvs,
    Settings,
}
impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Liked => "liked",
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Mvs => "mvs",
            Self::Settings => "settings",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MutationPhase {
    Sending,
    Verifying,
    Uncertain,
    Confirmed,
    Rejected,
    Unchanged,
    #[serde(rename = "accepted_current_state")]
    AcceptedCurrentState,
}
impl MutationPhase {
    pub fn pending(self) -> bool {
        matches!(self, Self::Sending | Self::Verifying | Self::Uncertain)
    }
}

/// Native persisted journal. The UI should receive a dedicated operation DTO.
#[derive(Clone, Serialize, Deserialize)]
pub struct MutationRecord {
    pub kind: ResourceKind,
    pub key: String,
    pub before: Value,
    pub desired: Value,
    pub created_at: f64,
    pub phase: MutationPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_code: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
}
impl MutationRecord {
    /// A confirmation is bound to this exact durable observation, not just ID.
    pub fn revision(&self) -> crate::Result<String> {
        use sha2::{Digest, Sha256};
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }
}
pub(super) fn key(kind: ResourceKind, key: &str) -> String {
    format!("{}:{key}", kind.as_str())
}
