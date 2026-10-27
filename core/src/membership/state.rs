//! M/224 C: membership status is derived from server-aligned expiry time.
use crate::{
    Error, Result,
    protocol::wire::serialization::{js_string, js_truthy},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const MEMBERSHIPS: [&str; 5] = [
    "associator",
    "redplus",
    "musicPackage",
    "albumVip",
    "voiceBookVip",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipState {
    pub is_never_vip_user: bool,
    pub is_vinyl_vip: bool,
    pub vinyl_vip_expire_time: f64,
    pub vinyl_vip_level: u32,
    pub is_plus_vip: bool,
    pub plus_vip_expire_time: f64,
    pub plus_vip_level: u32,
    pub is_music_package: bool,
    pub music_package_expire_time: f64,
    pub is_album_vip: bool,
    pub album_vip_expire_time: f64,
    pub is_voice_book_vip: bool,
    pub voice_book_vip_expire_time: f64,
    pub user_id: String,
    pub delta_expire_time_map: BTreeMap<String, f64>,
}
impl MembershipState {
    pub(crate) fn active_flags(&self) -> [bool; 5] {
        [
            self.is_vinyl_vip,
            self.is_plus_vip,
            self.is_music_package,
            self.is_album_vip,
            self.is_voice_book_vip,
        ]
    }
}
fn expiry(data: &Value, key: &str) -> Result<f64> {
    let value = &data[key]["expireTime"];
    if !js_truthy(value) {
        return Ok(0.0);
    }
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or(Error::Response("invalid membership expiry"))
}
fn level(data: &Value, key: &str) -> Result<u32> {
    let value = &data[key]["vipLevel"];
    if !js_truthy(value) {
        return Ok(0);
    }
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(Error::Response("invalid membership level"))
}
pub fn membership_state(data: &Value, now_ms: f64) -> Result<MembershipState> {
    if !now_ms.is_finite() || now_ms <= 0.0 {
        return Err(Error::InvalidInput(
            "positive server-aligned timestamp required",
        ));
    }
    if !data.is_object() {
        return Err(Error::Response("membership data must be an object"));
    }
    let expiry = MEMBERSHIPS
        .iter()
        .map(|key| expiry(data, key))
        .collect::<Result<Vec<_>>>()?;
    let deltas: BTreeMap<_, _> = MEMBERSHIPS
        .iter()
        .zip(&expiry)
        .map(|(key, expiry)| ((*key).into(), expiry - now_ms))
        .collect();
    Ok(MembershipState {
        is_never_vip_user: !MEMBERSHIPS.iter().any(|key| js_truthy(&data[*key])),
        is_vinyl_vip: expiry[0] > now_ms,
        vinyl_vip_expire_time: expiry[0],
        vinyl_vip_level: level(data, "associator")?,
        is_plus_vip: expiry[1] > now_ms,
        plus_vip_expire_time: expiry[1],
        plus_vip_level: level(data, "redplus")?,
        is_music_package: expiry[2] > now_ms,
        music_package_expire_time: expiry[2],
        is_album_vip: expiry[3] > now_ms,
        album_vip_expire_time: expiry[3],
        is_voice_book_vip: expiry[4] > now_ms,
        voice_book_vip_expire_time: expiry[4],
        user_id: data
            .get("userId")
            .filter(|v| !v.is_null())
            .map(js_string)
            .transpose()?
            .unwrap_or_default(),
        delta_expire_time_map: deltas,
    })
}
