use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingKey {
    ShareSetting,
    PlayRecordSetting,
    PersonalServiceSetting,
    AllowSubscriptionNotify,
    AllowLikedNotify,
    AllowNewFollowerNotify,
    AllowPeopleCanSeeMyPlayRecord,
}
impl SettingKey {
    pub const ALL: [Self; 7] = [
        Self::ShareSetting,
        Self::PlayRecordSetting,
        Self::PersonalServiceSetting,
        Self::AllowSubscriptionNotify,
        Self::AllowLikedNotify,
        Self::AllowNewFollowerNotify,
        Self::AllowPeopleCanSeeMyPlayRecord,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::ShareSetting => "shareSetting",
            Self::PlayRecordSetting => "playRecordSetting",
            Self::PersonalServiceSetting => "personalServiceSetting",
            Self::AllowSubscriptionNotify => "allowSubscriptionNotify",
            Self::AllowLikedNotify => "allowLikedNotify",
            Self::AllowNewFollowerNotify => "allowNewFollowerNotify",
            Self::AllowPeopleCanSeeMyPlayRecord => "allowPeopleCanSeeMyPlayRecord",
        }
    }
    pub fn parse(key: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.key() == key)
            .ok_or(Error::InvalidInput("unknown account setting"))
    }
    pub fn value(self, raw: &Value) -> Option<SettingValue> {
        let value: SettingValue = serde_json::from_value(raw.clone()).ok()?;
        SettingChange { key: self, value }.validate().ok()?;
        Some(value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    Boolean(bool),
    Number(u8),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingChange {
    pub key: SettingKey,
    pub value: SettingValue,
}
impl SettingChange {
    pub fn validate(&self) -> Result<()> {
        super::super::validate_settings(&self.patch())
    }
    pub fn patch(&self) -> Value {
        json!({self.key.key():self.value})
    }
}
/// Native snapshot. Only explicitly projected scalar fields may cross IPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub revision: String,
    pub fetched_at: u64,
    pub values: Value,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPhase {
    Pending,
    Confirmed,
    Rejected,
    Uncertain,
    Unchanged,
    Cancelled,
    AcceptedCurrentState,
}
impl SettingsPhase {
    pub fn unresolved(self) -> bool {
        matches!(self, Self::Pending | Self::Uncertain)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsMutation {
    pub id: String,
    pub change: SettingChange,
    pub expected_revision: String,
    pub before: Option<SettingValue>,
    pub phase: SettingsPhase,
    pub created_at: u64,
    pub last_read_at: Option<u64>,
    pub read_revision: Option<String>,
    pub observed: Option<SettingValue>,
    pub service_code: Option<i64>,
}
