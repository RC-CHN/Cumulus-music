use crate::{
    client::dto::TrackView,
    listen_together::{RoomRole, SharedMode, TogetherPhase},
};
use serde::{Deserialize, Serialize};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherMemberView {
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub is_self: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherRoomView {
    pub id: String,
    pub role: RoomRole,
    pub members: Vec<TogetherMemberView>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherOperationView {
    pub id: String,
    pub action: &'static str,
    pub phase: TogetherPhase,
    pub acknowledged: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherView {
    pub room: Option<TogetherRoomView>,
    pub tracks: Vec<TrackView>,
    pub mode: SharedMode,
    pub target_id: Option<String>,
    pub progress_ms: Option<u64>,
    pub playing: bool,
    pub attached: bool,
    pub realtime_ready: bool,
    pub connection: crate::listen_together::transport::ConnectionView,
    pub pending: Vec<TogetherOperationView>,
    pub sync_error: Option<String>,
    pub checked_at_ms: Option<u64>,
    pub local_recovery: bool,
    pub ending: bool,
    pub lifecycle_notice: Option<String>,
    pub shared_phase: Option<crate::listen_together::IncomingPhase>,
    pub common_access: Vec<TogetherAccessView>,
    pub native_target_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherAccessView {
    pub track_id: String,
    pub can_play: bool,
    pub reason: Option<String>,
    pub preview: Option<crate::listen_together::Preview>,
    pub limited_member: crate::listen_together::LimitedMember,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TogetherCommand {
    Create,
    Attach { room_id: String },
    Reconnect { room_id: String },
    Publish { room_id: String },
    Select { room_id: String, song_id: String },
    Remove { room_id: String, song_id: String },
    End { room_id: String },
    Reconcile { operation_id: String },
    RestoreLocal,
}
