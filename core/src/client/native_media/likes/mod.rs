//! Resource identity is separate from transport intent: pausing a track does
//! not invalidate its like button, while account/selection changes do.
mod actions;
mod view;
use crate::{
    Error, Result,
    account::{
        SessionStatus, SessionToken,
        resources::{AccountResources, ResourceKind},
    },
    client::{MusicClient, focus::Owner},
    podcast::{PodcastAction, PodcastLibrary, PodcastScope, PodcastService},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NativeLikeResource {
    Song(String),
    Voice(String),
}
impl NativeLikeResource {
    pub fn id(&self) -> &str {
        match self {
            Self::Song(id) | Self::Voice(id) => id,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeLikeSelection {
    token: SessionToken,
    entry_id: String,
    pub resource: NativeLikeResource,
}
impl NativeLikeSelection {
    pub fn epoch(&self) -> String {
        self.token.generation.to_string()
    }
    fn check(&self, client: &MusicClient) -> Result<()> {
        if client.native_like_selection()?.as_ref() != Some(self) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeLikeSnapshot {
    pub selection: NativeLikeSelection,
    pub observed: Option<bool>,
    pub pending: bool,
}
