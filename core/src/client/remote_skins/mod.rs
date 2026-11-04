//! The recovered PC skin catalogue is read only. Images are served from an
//! account-bound native snapshot, not arbitrary renderer-supplied URLs.
mod artwork;

use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::{SessionStatus, SessionToken},
    download::transfer::{MediaBackend, MediaUrl, ReqwestMediaBackend},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

const LIST: &str = "/api/music-vip-center/app/pc/skin/list";
const MAX_GROUPS: usize = 24;
const MAX_CARDS: usize = 160;
const REQUEST_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSkinImage {
    Cover,
    Preview,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SkinEntitlement {
    Vinyl,
    SuperVinyl,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum SkinCorner {
    Popular,
    New,
    Limited,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkinCard {
    key: u16,
    name: String,
    entitlement: Option<SkinEntitlement>,
    corner: Option<SkinCorner>,
    has_cover: bool,
    has_preview: bool,
}
#[derive(Serialize)]
struct SkinGroup {
    name: String,
    cards: Vec<SkinCard>,
}
#[derive(Serialize)]
pub struct RemoteSkinCatalog {
    revision: String,
    groups: Vec<SkinGroup>,
}
struct CardArtwork {
    cover: Option<MediaUrl>,
    preview: Option<MediaUrl>,
}
struct Snapshot {
    token: SessionToken,
    revision: String,
    artwork: Vec<CardArtwork>,
}
pub(super) struct RemoteSkinHost {
    snapshot: Mutex<Option<Snapshot>>,
    backend: Mutex<Option<Arc<dyn MediaBackend>>>,
    images: Semaphore,
    reads: tokio::sync::Mutex<()>,
}

impl Default for RemoteSkinHost {
    fn default() -> Self {
        Self {
            snapshot: Mutex::new(None),
            backend: Mutex::new(None),
            images: Semaphore::new(3),
            reads: tokio::sync::Mutex::new(()),
        }
    }
}
impl RemoteSkinHost {
    fn image(
        &self,
        token: &SessionToken,
        revision: &str,
        key: u16,
        kind: RemoteSkinImage,
    ) -> Result<MediaUrl> {
        let slot = self.snapshot.lock().map_err(|_| Error::LockPoisoned)?;
        let snapshot = slot
            .as_ref()
            .filter(|s| s.token == *token && s.revision == revision)
            .ok_or(Error::StaleResponse)?;
        let entry = snapshot
            .artwork
            .get(usize::from(key))
            .ok_or(Error::InvalidInput("unknown skin image"))?;
        match kind {
            RemoteSkinImage::Cover => entry.cover.clone(),
            RemoteSkinImage::Preview => entry.preview.clone(),
        }
        .ok_or(Error::InvalidInput("skin image unavailable"))
    }

    fn backend(&self) -> Result<Arc<dyn MediaBackend>> {
        let mut slot = self.backend.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(backend) = &*slot {
            return Ok(backend.clone());
        }
        let backend = Arc::new(ReqwestMediaBackend::new(Duration::from_secs(5))?);
        *slot = Some(backend.clone());
        Ok(backend)
    }
}

fn name(value: &Value) -> Option<String> {
    let name = value.as_str()?.trim();
    (!name.is_empty()
        && name.chars().count() <= 120
        && !name.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }))
    .then(|| name.to_owned())
}

fn project(response: &Value, revision: String) -> Result<(RemoteSkinCatalog, Vec<CardArtwork>)> {
    let data = &response["data"];
    if !data.is_null() && !data.is_object() {
        return Err(Error::Response("invalid skin catalogue data"));
    }
    let tabs = match &data["tabs"] {
        Value::Null => &[][..],
        Value::Array(tabs) if tabs.len() <= MAX_GROUPS => tabs,
        _ => return Err(Error::Response("invalid skin catalogue groups")),
    };
    let mut groups = Vec::new();
    let mut artwork = Vec::new();
    let mut total = 0usize;
    for tab in tabs {
        let Some(rows) = tab["detailVoList"].as_array() else {
            continue;
        };
        total = total.saturating_add(rows.len());
        if total > MAX_CARDS {
            return Err(Error::Response("skin catalogue exceeds limit"));
        }
        let Some(group) = name(&tab["name"]) else {
            continue;
        };
        let mut cards = Vec::new();
        for row in rows {
            let Some(name) = name(&row["name"]) else {
                continue;
            };
            let cover = artwork::url(&row["url"]);
            let preview = artwork::url(&row["previewUrl"]);
            cards.push(SkinCard {
                key: u16::try_from(artwork.len()).expect("catalogue limit fits a skin image key"),
                name,
                entitlement: match row["type"].as_i64() {
                    Some(6) => Some(SkinEntitlement::Vinyl),
                    Some(15) => Some(SkinEntitlement::SuperVinyl),
                    _ => None,
                },
                corner: match row["cornerType"].as_i64() {
                    Some(2) => Some(SkinCorner::Popular),
                    Some(3) => Some(SkinCorner::New),
                    Some(4) => Some(SkinCorner::Limited),
                    _ => None,
                },
                has_cover: cover.is_some(),
                has_preview: preview.is_some(),
            });
            artwork.push(CardArtwork { cover, preview });
        }
        if !cards.is_empty() {
            groups.push(SkinGroup { name: group, cards });
        }
    }
    Ok((RemoteSkinCatalog { revision, groups }, artwork))
}

impl MusicClient {
    /// Injectable credential-free media only; the API continues to use the
    /// active account session. Used by the closed WebView fixture.
    pub fn with_remote_skin_images(self, backend: Arc<dyn MediaBackend>) -> Self {
        *self
            .remote_skins
            .backend
            .lock()
            .expect("new skin image backend") = Some(backend);
        self
    }

    fn skin_token(&self, epoch: &str) -> Result<SessionToken> {
        let snapshot = self.session.snapshot()?;
        if snapshot.token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        if snapshot.status != SessionStatus::Authenticated {
            return Err(Error::State("remote skins require a verified account"));
        }
        self.session.with_token(&snapshot.token, |_| Ok(()))?;
        Ok(snapshot.token)
    }

    pub async fn remote_skin_catalog(&self, epoch: &str) -> Result<AccountData<RemoteSkinCatalog>> {
        let token = self.skin_token(epoch)?;
        let _read = self.remote_skins.reads.lock().await;
        self.session.with_token(&token, |_| Ok(()))?;
        let mut generation = self.session.subscribe()?;
        let request = self.session.run_for(&token, |api, _| async move {
            tokio::time::timeout(REQUEST_LIMIT, api.call(LIST, json!({}), false, &[200]))
                .await
                .map_err(|_| Error::Transport("skin catalogue timed out".into()))?
        });
        let response = tokio::select! {
            biased;
            _ = generation.wait_for(|owner| *owner != token) => return Err(Error::StaleResponse),
            response = request => response?,
        };
        if response["code"] != 200 {
            return Err(Error::Response("invalid skin catalogue status"));
        }
        let revision = uuid::Uuid::new_v4().simple().to_string();
        let (catalog, artwork) = project(&response, revision.clone())?;
        self.session.with_token(&token, |_| {
            *self
                .remote_skins
                .snapshot
                .lock()
                .map_err(|_| Error::LockPoisoned)? = Some(Snapshot {
                token: token.clone(),
                revision,
                artwork,
            });
            Ok(AccountData {
                epoch: epoch.into(),
                value: catalog,
            })
        })
    }

    pub async fn remote_skin_image(
        &self,
        epoch: &str,
        revision: &str,
        key: u16,
        kind: RemoteSkinImage,
    ) -> Result<AccountData<String>> {
        let token = self.skin_token(epoch)?;
        let _permit = self
            .remote_skins
            .images
            .try_acquire()
            .map_err(|_| Error::State("skin images are busy"))?;
        let url = self.session.with_token(&token, |_| {
            self.remote_skins.image(&token, revision, key, kind)
        })?;
        let backend = self.remote_skins.backend()?;
        let mut generation = self.session.subscribe()?;
        let read = tokio::time::timeout(REQUEST_LIMIT, artwork::download(backend.as_ref(), url));
        let bytes = tokio::select! {
            biased;
            _ = generation.wait_for(|owner| *owner != token) => return Err(Error::StaleResponse),
            bytes = read => bytes.map_err(|_| Error::Transport("skin image timed out".into()))??,
        };
        self.session.with_token(&token, |_| {
            self.remote_skins
                .image(&token, revision, key, kind)
                .map(|_| ())
        })?;
        let value = tokio::task::spawn_blocking(move || artwork::render(&bytes, kind))
            .await
            .map_err(|_| Error::Media("skin image worker failed"))??;
        self.session.with_token(&token, |_| {
            self.remote_skins.image(&token, revision, key, kind)?;
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

#[cfg(test)]
mod tests;
