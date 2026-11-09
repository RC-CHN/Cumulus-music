//! PC actionPlay trial context for an ordinary playlist or a song from it.
use super::MusicClient;
use crate::{Result, account::SessionToken, protocol::api::identifier};
use serde_json::Value;

pub(super) const RADAR: i32 = 37;
pub(super) const LIKED: i32 = 43;

// M/91.a, 3.1.42.205594. The playlist ID is the selector, not a title, tag,
// specialType=100, or a trial claim from the WebView.
const RADAR_PLAYLISTS: &[&str] = &[
    "9102252203",
    "8902540200",
    "8892529201",
    "8819359201",
    "8805180200",
    "8402996200",
    "5362359247",
    "5341776086",
    "5327906368",
    "5320167908",
    "5300458264",
    "3136952023",
    "2829896389",
    "2829920189",
    "2829883282",
    "2829816518",
];

pub(super) fn inferred_playlist_source(source: &Value) -> bool {
    source["kind"] == "playlist"
        && source["trial_mode"]
            .as_i64()
            .is_some_and(|mode| [i64::from(RADAR), i64::from(LIKED)].contains(&mode))
}

impl MusicClient {
    pub(super) fn ordinary_playlist_trial(
        &self,
        token: &SessionToken,
        id: &str,
        detail: &Value,
    ) -> Result<Option<i32>> {
        let Some(uid) = token.uid.as_deref() else {
            return Ok(None);
        };
        if identifier(&detail["id"]).ok().as_deref() != Some(id) {
            return Ok(None);
        }
        // M/3085 uses the already observed starPlaylistId. A detail tag alone
        // cannot give an arbitrary playlist the account's liked context.
        if self.intelligence_liked_hint(token)?.as_deref() == Some(id) {
            let compatible_owner = [&detail["userId"], &detail["creator"]["userId"]]
                .iter()
                .all(|owner| owner.is_null() || identifier(owner).ok().as_deref() == Some(uid));
            let special = &detail["specialType"];
            if compatible_owner && (special.is_null() || special.as_i64() == Some(5)) {
                return Ok(Some(LIKED));
            }
        }
        Ok(RADAR_PLAYLISTS.contains(&id).then_some(RADAR))
    }
}
