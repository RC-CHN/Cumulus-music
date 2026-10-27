//! Official audio home Scene FM: a finite voice queue, separate from music FM.
mod actions;
mod view;
use super::dto::EpisodeView;
use crate::{Error, Result, account::SessionToken};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct SceneFmHost {
    attempted: Mutex<Option<SessionToken>>,
    operation: tokio::sync::Mutex<()>,
}
impl SceneFmHost {
    fn attempt(&self, token: &SessionToken, automatic: bool) -> Result<bool> {
        let mut used = self.attempted.lock().map_err(|_| Error::LockPoisoned)?;
        if automatic && used.as_ref() == Some(token) {
            return Ok(false);
        }
        *used = Some(token.clone());
        Ok(true)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneFmCategoryView {
    pub id: String,
    pub name: String,
    pub label: String,
    pub count: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneFmView {
    pub revision: Option<String>,
    pub active_id: Option<String>,
    pub scenes: Vec<SceneFmCategoryView>,
    pub episode: Option<EpisodeView>,
    pub active: bool,
    pub playing: bool,
    pub pending: bool,
    pub autoplay: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneFmCommand {
    Select { revision: String, scene_id: String },
    Play { revision: String },
    Toggle { revision: String },
    Automatic { revision: String },
    Autoplay { enabled: bool },
    Cancel { revision: String },
}
fn revision(v: &str) -> Result<u64> {
    let n = v
        .parse::<u64>()
        .map_err(|_| Error::InvalidInput("invalid scene FM revision"))?;
    if n.to_string() != v {
        return Err(Error::InvalidInput("invalid scene FM revision"));
    }
    Ok(n)
}
