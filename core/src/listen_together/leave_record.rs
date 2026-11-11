use super::{Room, RoomRole};
use serde_json::Value;

/// M/3043 displays these four values from the direct M/75 end/v2 reply.
#[derive(Clone, Debug)]
pub(super) struct LeaveSummary {
    pub this_listen_count: u64,
    pub this_duration: u64,
    pub total_listen_count: u64,
    pub total_duration: u64,
}

impl LeaveSummary {
    pub(super) fn from_official(info: &Value) -> Option<Self> {
        Some(Self {
            this_listen_count: number(&info["thisListenCount"], false)?,
            this_duration: number(&info["thisDuration"], true)?,
            total_listen_count: number(&info["totalListenCount"], false)?,
            total_duration: number(&info["totalDuration"], true)?,
        })
    }
}

fn number(value: &Value, duration: bool) -> Option<u64> {
    const JS_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    let value = value.as_f64()?;
    // M/103.i floors duration in seconds. Counts remain whole numbers.
    ((0.0..=JS_SAFE_INTEGER).contains(&value) && (duration || value.fract() == 0.0))
        .then_some(value.floor() as u64)
}

/// Transient result only; neither the room payload nor share URL is retained.
#[derive(Clone, Debug)]
pub(crate) struct DirectLeaveRecord {
    pub this_listen_count: u64,
    pub this_duration: u64,
    pub total_listen_count: u64,
    pub total_duration: u64,
    pub self_avatar_url: Option<String>,
    pub peer_avatar_url: Option<String>,
}

impl DirectLeaveRecord {
    pub(super) fn from_ack(
        summary: LeaveSummary,
        room: &Room,
        role: Option<RoomRole>,
        owner: &str,
    ) -> Option<Self> {
        if !matches!(role, Some(RoomRole::TogetherOwner | RoomRole::Together))
            || room.role_for(owner).ok() != role
        {
            return None;
        }
        let avatar = |member: &super::Member| {
            crate::protocol::artwork::square(&Value::from(member.avatar_url.as_deref()?))
        };
        let me = room.members.iter().find(|m| m.user_id == owner);
        let mut peers = room.members.iter().filter(|m| m.user_id != owner);
        let peer = peers.next().filter(|_| peers.next().is_none());
        Some(Self {
            this_listen_count: summary.this_listen_count,
            this_duration: summary.this_duration,
            total_listen_count: summary.total_listen_count,
            total_duration: summary.total_duration,
            self_avatar_url: me.and_then(avatar),
            peer_avatar_url: peer.and_then(avatar),
        })
    }
}
