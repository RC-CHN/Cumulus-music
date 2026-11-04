//! Resource namespaces used by queue edits and UI current-row indicators.
use super::PlayerResourceKind;
use serde_json::Value;
pub(super) fn kind(resource: &Value) -> PlayerResourceKind {
    for (key, kind) in [
        ("download_task_id", PlayerResourceKind::Downloaded),
        ("local_id", PlayerResourceKind::Local),
        ("cloud_song_id", PlayerResourceKind::Cloud),
        ("voice_id", PlayerResourceKind::Voice),
    ] {
        if resource.get(key).is_some() {
            return kind;
        }
    }
    PlayerResourceKind::Catalog
}
pub(super) fn online_song(resource: &Value, id: &str) -> bool {
    kind(resource) == PlayerResourceKind::Catalog && resource["id"].as_str() == Some(id)
}
pub(super) fn collection_key(
    resource: &Value,
    downloaded: bool,
) -> Option<(PlayerResourceKind, &str)> {
    let kind = kind(resource);
    let field = match (downloaded, kind) {
        (true, PlayerResourceKind::Downloaded) => "download_task_id",
        (false, PlayerResourceKind::Catalog) => "id",
        (false, PlayerResourceKind::Local) => "local_id",
        (false, PlayerResourceKind::Cloud) => "cloud_song_id",
        (false, PlayerResourceKind::Voice) => "voice_id",
        _ => return None,
    };
    resource[field].as_str().map(|id| (kind, id))
}
