//! M/342 getAudioEffectAuthResult and M/85 authorization types.
use crate::protocol::wire::serialization::js_truthy;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authorization {
    pub can_use: bool,
    pub can_show: Option<bool>,
    pub can_use_type: Option<u32>,
    pub required_membership: Option<u32>,
    pub rejection_reason: Option<u32>,
    pub catalogue_trial_active: bool,
}
fn uint(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|v| u32::try_from(v).ok())
}
impl Authorization {
    pub fn from_official(value: &Value) -> Option<Self> {
        if !value.is_object() {
            return None;
        }
        Some(Self {
            // Malformed truthy strings never grant activation in the Rust API.
            can_use: value["authResult"]["canUse"].as_bool() == Some(true),
            can_show: value["authResult"]["canShow"].as_bool(),
            can_use_type: uint(&value["authResult"]["canUseType"]),
            required_membership: uint(&value["interestsVipType"]),
            rejection_reason: uint(&value["authResult"]["canNoUseReasonCode"]),
            catalogue_trial_active: js_truthy(&value["authResult"]["trialDto"]["trialStatus"]),
        })
    }
    pub fn is_trial(&self) -> bool {
        matches!(self.can_use_type, Some(50 | 51))
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct HostRights {
    pub anonymous: bool,
    pub vinyl_vip: bool,
    pub plus_vip: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredMembership {
    Vip,
    Svip,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessAction {
    Allow,
    Login,
    Upgrade,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessDecision {
    pub action: AccessAction,
    pub membership: Option<RequiredMembership>,
    pub trial_use: bool,
    pub degraded: bool,
}
pub enum AuthQuery<'a> {
    /// A successful response with no matching record is confirmed denial.
    Confirmed(Option<&'a Authorization>),
    Unavailable,
}
fn tier(value: Option<&Authorization>) -> Option<RequiredMembership> {
    match value.and_then(|v| v.required_membership) {
        Some(6) => Some(RequiredMembership::Vip),
        Some(15) => Some(RequiredMembership::Svip),
        _ => None,
    }
}
pub fn decide_access(
    host: HostRights,
    catalogue: Option<&Authorization>,
    query: AuthQuery<'_>,
) -> AccessDecision {
    let mut membership = tier(catalogue);
    if host.anonymous && membership.is_some() {
        return AccessDecision {
            action: AccessAction::Login,
            membership: None,
            trial_use: false,
            degraded: false,
        };
    }
    let (can_use, trial_use, degraded) = match query {
        AuthQuery::Confirmed(auth) => {
            membership = tier(auth);
            (
                auth.is_some_and(|v| v.can_use),
                auth.is_some_and(Authorization::is_trial),
                false,
            )
        }
        AuthQuery::Unavailable => {
            let allow = match membership {
                None => true,
                Some(RequiredMembership::Vip) => host.vinyl_vip || host.plus_vip,
                Some(RequiredMembership::Svip) => host.plus_vip,
            } || catalogue.is_some_and(|v| v.catalogue_trial_active);
            (allow, false, true)
        }
    };
    if can_use {
        AccessDecision {
            action: AccessAction::Allow,
            membership,
            trial_use,
            degraded,
        }
    } else if host.anonymous {
        AccessDecision {
            action: AccessAction::Login,
            membership: None,
            trial_use: false,
            degraded,
        }
    } else {
        AccessDecision {
            action: AccessAction::Upgrade,
            membership: Some(membership.unwrap_or(RequiredMembership::Vip)),
            trial_use: false,
            degraded,
        }
    }
}
