use super::{CatalogQuery, request};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    protocol::api::identifier,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const CACHE: &str = "catalog-detail-pages-v1";
const ALBUM_CACHE: &str = "catalog-album-pages-v2";
const ALBUM_PLAYBACK_MARKER: &str = "catalog-album-playback-marker-v1";

pub(crate) fn album_duration(song: &Value) -> u64 {
    song["dt"]
        .as_u64()
        .filter(|n| *n > 0)
        .or_else(|| song["duration"].as_u64())
        // Preserve exact numeric ordering on both sides of IPC.
        .filter(|n| *n <= 9_007_199_254_740_991)
        .unwrap_or(0)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedCatalogPage {
    pub query: CatalogQuery,
    pub fetched_at: u64,
    pub revision: String,
    #[serde(default)]
    pub album_read: Option<crate::catalog::AlbumReadState>,
    /// Account-owned native metadata, never a frontend DTO.
    pub response: Value,
}
impl SavedCatalogPage {
    pub(crate) fn album_playback_revision(&self) -> Result<String> {
        if !matches!(self.query, CatalogQuery::Album { .. }) {
            return Err(Error::InvalidInput("not an album snapshot"));
        }
        let songs = self
            .songs()
            .ok_or(Error::Response("missing album tracks"))?;
        if songs.len() > 50_000 {
            return Err(Error::InvalidInput("album too large"));
        }
        let mut stamp = Sha256::new();
        // Existing catalog revision binds the album and original ID order.
        stamp.update(self.revision.as_bytes());
        for song in songs {
            let name = song["name"].as_str().unwrap_or_default();
            let album = song.get("al").or_else(|| song.get("album"));
            let album_name = album
                .and_then(|value| value["name"].as_str())
                .unwrap_or_default();
            let artists = song
                .get("ar")
                .or_else(|| song.get("artists"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|artist| artist["name"].as_str().unwrap_or_default())
                .collect::<Vec<_>>();
            stamp.update(serde_json::to_vec(&(
                album_duration(song),
                name,
                artists,
                album_name,
            ))?);
        }
        Ok(hex::encode(stamp.finalize()))
    }
    pub(crate) fn verify_song_order(&self, songs: &[Value]) -> Result<()> {
        let field = match self.query {
            CatalogQuery::Artist { .. } => "hotSongs",
            CatalogQuery::ArtistSongs { .. } | CatalogQuery::Album { .. } => "songs",
            _ => return Err(Error::InvalidInput("catalog page has no song order")),
        };
        let response = json!({field:songs});
        if self.revision != revision(&self.query, &response)? {
            return Err(Error::QueueChanged);
        }
        Ok(())
    }
    pub fn songs(&self) -> Option<&[Value]> {
        let field = match self.query {
            CatalogQuery::Artist { .. } => "hotSongs",
            CatalogQuery::ArtistSongs { .. } | CatalogQuery::Album { .. } => "songs",
            _ => return None,
        };
        self.response[field].as_array().map(Vec::as_slice)
    }
}
fn revision(query: &CatalogQuery, response: &Value) -> Result<String> {
    let field = match query {
        CatalogQuery::Artist { .. } => Some("hotSongs"),
        CatalogQuery::ArtistSongs { .. } | CatalogQuery::Album { .. } => Some("songs"),
        _ => None,
    };
    let content = if let Some(field) = field {
        json!(
            response[field]
                .as_array()
                .ok_or(Error::Response("missing catalog tracks"))?
                .iter()
                .map(|s| identifier(&s["id"]))
                .collect::<Result<Vec<_>>>()?
        )
    } else {
        response.clone()
    };
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&json!([
        query, content
    ]))?)))
}
pub(crate) struct AlbumPlaybackFence {
    session: Arc<AccountSession>,
    token: SessionToken,
    store: Arc<StateStore>,
    key: String,
    expected: String,
}

impl AlbumPlaybackFence {
    pub(crate) fn is_current(&self) -> bool {
        self.session
            .with_token(&self.token, |owner| {
                Ok(self
                    .store
                    .get(owner, ALBUM_PLAYBACK_MARKER, &self.key)?
                    .as_ref()
                    .and_then(Value::as_str)
                    == Some(self.expected.as_str()))
            })
            .unwrap_or(false)
    }
}

fn decode_cached(query: &CatalogQuery, value: Value) -> Result<SavedCatalogPage> {
    let saved: SavedCatalogPage = serde_json::from_value(value)?;
    if saved.query != *query {
        return Err(Error::State("catalog cache identity mismatch"));
    }
    request::validate(query, &saved.response)?;
    if saved.revision != revision(query, &saved.response)? {
        return Err(Error::State("catalog cache revision mismatch"));
    }
    Ok(saved)
}

pub struct CatalogDetails {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl CatalogDetails {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn key(query: &CatalogQuery) -> Result<String> {
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(query)?)))
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        query: &CatalogQuery,
    ) -> Result<Option<SavedCatalogPage>> {
        let key = Self::key(query)?;
        self.session.with_token(token, |uid| {
            let raw = if matches!(query, CatalogQuery::Album { .. }) {
                self.store
                    .read_snapshot(|tx| match tx.get(uid, ALBUM_CACHE, &key)? {
                        Some(current) => Ok(Some(current)),
                        None => tx.get(uid, CACHE, &key),
                    })?
            } else {
                self.store.get(uid, CACHE, &key)?
            };
            raw.map(|value| decode_cached(query, value)).transpose()
        })
    }

    /// Validate a renderer revision and retain only a small native lookup for
    /// the actor. Any native process that changes the current album snapshot
    /// commits its marker in the same transaction as that snapshot.
    pub(crate) fn album_playback_fence(
        &self,
        token: &SessionToken,
        query: &CatalogQuery,
        expected: &str,
    ) -> Result<AlbumPlaybackFence> {
        if !matches!(query, CatalogQuery::Album { .. })
            || expected.len() != 64
            || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::QueueChanged);
        }
        let key = Self::key(query)?;
        let fence = AlbumPlaybackFence {
            session: self.session.clone(),
            token: token.clone(),
            store: self.store.clone(),
            key,
            expected: expected.into(),
        };
        self.session.with_token(token, |owner| {
            if let Some(marker) = self.store.get(owner, ALBUM_PLAYBACK_MARKER, &fence.key)? {
                return if marker.as_str() == Some(expected) {
                    Ok(())
                } else {
                    Err(Error::QueueChanged)
                };
            }
            // One-time upgrade for a validated old recovery snapshot. The
            // versioned cache also prevents an older binary from replacing
            // the authoritative row without its required marker.
            self.store.transaction(|tx| {
                if let Some(marker) = tx.get(owner, ALBUM_PLAYBACK_MARKER, &fence.key)? {
                    return if marker.as_str() == Some(expected) {
                        Ok(())
                    } else {
                        Err(Error::QueueChanged)
                    };
                }
                let raw = match tx.get(owner, ALBUM_CACHE, &fence.key)? {
                    Some(current) => current,
                    None => tx
                        .get(owner, CACHE, &fence.key)?
                        .ok_or(Error::QueueChanged)?,
                };
                let saved = decode_cached(query, raw)?;
                let marker = saved.album_playback_revision()?;
                if marker != expected {
                    return Err(Error::QueueChanged);
                }
                tx.put(
                    owner,
                    ALBUM_CACHE,
                    &fence.key,
                    &serde_json::to_value(&saved)?,
                )?;
                tx.put(owner, ALBUM_PLAYBACK_MARKER, &fence.key, &json!(marker))
            })
        })?;
        Ok(fence)
    }
    /// M/15's g has enable:false. A recovery snapshot must not suppress this
    /// request on the next page visit or authorize playback from old rights.
    pub async fn page_for(
        &self,
        token: &SessionToken,
        query: &CatalogQuery,
    ) -> Result<SavedCatalogPage> {
        let key = Self::key(query)?;
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let _lock =
            OperationLock::acquire(self.store.directory(), CACHE, &format!("{owner}:{key}"))?;
        let (response, album_read) = self
            .session
            .run_for(
                token,
                |api, _| async move { request::fetch(api, query).await },
            )
            .await?;
        let saved = SavedCatalogPage {
            query: query.clone(),
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            revision: revision(query, &response)?,
            response,
            album_read,
        };
        let raw = serde_json::to_value(&saved)?;
        let marker = if matches!(query, CatalogQuery::Album { .. }) {
            Some(json!(saved.album_playback_revision()?))
        } else {
            None
        };
        self.session.with_token(token, |uid| match marker {
            Some(marker) => self.store.transaction(|tx| {
                tx.put(uid, ALBUM_CACHE, &key, &raw)?;
                tx.put(uid, ALBUM_PLAYBACK_MARKER, &key, &marker)
            }),
            None => self.store.put(uid, CACHE, &key, &raw),
        })?;
        Ok(saved)
    }
}
