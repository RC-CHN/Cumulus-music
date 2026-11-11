//! Count preparation for one observed song in a dedicated song or lyric search tab.
use super::{MusicClient, SearchCategory, SearchItem, SearchResultView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::{AccountData, TrackView},
    comments::CommentApi,
    protocol::{
        api::{Api, SearchSongCommentSource, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;

const PAGE_LIMIT: usize = 16;
const ROW_LIMIT: usize = 128;
const LIFETIME: Duration = Duration::from_secs(30 * 60);
const COUNT_LIMIT: Duration = Duration::from_millis(1500);

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchSongCommentMenu {
    Eligible { receipt: String },
    Hidden,
}

// M556 comment eligibility differs from Share: fee/negative copyright are
// immaterial here. A mixed private/local source still cannot authorize a
// public directory identity by presenting a numeric-looking display ID.
pub(super) fn observe(raw: &Value, track: &TrackView) -> Option<String> {
    let id = identifier(&raw["id"]).ok()?;
    if id != track.id
        || id.len() > 128
        || id.starts_with('0')
        || track.local
        || track.missing
        || track.voice_id.is_some()
        || !public_song_source(raw, &id)
    {
        return None;
    }
    Some(id)
}

fn public_song_source(raw: &Value, id: &str) -> bool {
    super::super::mapping::catalog_song_comment_eligible(raw)
        && raw["songType"].as_f64() != Some(1.0)
        && ![
            "fileId",
            "filename",
            "localId",
            "local_id",
            "localTrack",
            "offlineId",
            "onlineTrack",
            "cloudSongId",
            "cloud_song_id",
            "programId",
            "voiceId",
            "voice_id",
        ]
        .iter()
        .any(|field| js_truthy(&raw[*field]))
        && (!js_truthy(&raw["onlineTrackId"])
            || identifier(&raw["onlineTrackId"]).ok().as_deref() == Some(id))
}

pub(super) fn observe_lyric(resource: &Value, track: &TrackView) -> Option<String> {
    // M15.Wn spreads the outer row and then its song. A matched excerpt is
    // never itself a song identity; do not bind an ambiguous or private row.
    let id = observe(resource.get("song")?, track)?;
    if !public_song_source(resource, &id)
        || ["id", "songId", "trackId"].iter().any(|field| {
            resource
                .get(field)
                .filter(|value| !value.is_null())
                .is_some_and(|value| identifier(value).ok().as_deref() != Some(id.as_str()))
        })
    {
        return None;
    }
    Some(id)
}

struct Page {
    source: SearchSongCommentSource,
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    issued: Instant,
    rows: HashMap<Uuid, String>,
}

impl Page {
    fn owns(&self, token: &SessionToken, api: &Weak<dyn Api>, credentials: Option<&str>) -> bool {
        self.token == *token
            && self.api.ptr_eq(api)
            && self.credentials.as_deref() == credentials
            && self.issued.elapsed() < LIFETIME
    }
}

#[derive(Default)]
pub(super) struct SearchSongCommentPages(Mutex<VecDeque<Page>>);

impl SearchSongCommentPages {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<Option<String>>,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let candidates = std::mem::take(&mut view.song_comment_rows);
        let [section] = view.sections.as_mut_slice() else {
            return Ok(());
        };
        let source = match view.query.category {
            SearchCategory::Songs => SearchSongCommentSource::Songs,
            SearchCategory::Lyrics => SearchSongCommentSource::Lyrics,
            _ => return Ok(()),
        };
        if token.uid.is_none()
            || section.category != Some(view.query.category)
            || candidates.len() != section.items.len()
        {
            return Ok(());
        }
        let credentials = credentials.filter(|before| {
            api.search_privilege_credential_revision().ok().as_ref() == Some(before)
        });
        let mut rows = HashMap::new();
        for (item, candidate) in section.items.iter_mut().zip(candidates) {
            if let SearchItem::Song {
                track,
                song_comment_menu,
                ..
            } = item
            {
                *song_comment_menu = match candidate {
                    Some(id) if id == track.id => {
                        if credentials.is_some() && rows.len() < ROW_LIMIT {
                            let receipt = Uuid::new_v4();
                            rows.insert(receipt, id);
                            Some(Box::new(SearchSongCommentMenu::Eligible {
                                receipt: receipt.simple().to_string(),
                            }))
                        } else {
                            None
                        }
                    }
                    _ => Some(Box::new(SearchSongCommentMenu::Hidden)),
                };
            }
        }
        if let Some(credentials) = credentials
            && !rows.is_empty()
        {
            let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
            let api = Arc::downgrade(api);
            pages.retain(|page| page.owns(token, &api, credentials.as_deref()));
            if pages.len() >= PAGE_LIMIT {
                pages.pop_front();
            }
            pages.push_back(Page {
                source,
                token: token.clone(),
                api,
                credentials,
                issued: Instant::now(),
                rows,
            });
        }
        Ok(())
    }

    fn require(
        &self,
        token: &SessionToken,
        api: &Weak<dyn Api>,
        credentials: Option<&str>,
        receipt: Uuid,
    ) -> Result<(SearchSongCommentSource, String)> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .find_map(|page| {
                if page.owns(token, api, credentials) {
                    page.rows.get(&receipt).map(|id| (page.source, id.clone()))
                } else {
                    None
                }
            })
            .ok_or(Error::QueueChanged)
    }
}

impl MusicClient {
    pub async fn search_song_row_comment_count(
        &self,
        epoch: &str,
        receipt: &str,
    ) -> Result<AccountData<Option<u64>>> {
        let token = self.player_token(epoch)?;
        if receipt.len() != 32
            || !receipt
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::QueueChanged);
        }
        let receipt = Uuid::try_parse(receipt).map_err(|_| Error::QueueChanged)?;
        let api = self.session.native_api(&token)?;
        let credentials = api.search_privilege_credential_revision()?;
        let weak = Arc::downgrade(&api);
        let current = || {
            if self.login_pending()? || api.search_privilege_credential_revision()? != credentials {
                return Err(Error::StaleResponse);
            }
            self.session.require_api(&token, &api)?;
            self.search
                .song_comment_pages
                .require(&token, &weak, credentials.as_deref(), receipt)
        };
        let (source, id) = current()?;
        let permission = || current().map(|_| ());
        let comments = CommentApi::new(api.clone());
        let result = tokio::time::timeout(
            COUNT_LIMIT,
            comments.search_song_count(source, id, credentials.as_deref(), &permission),
        )
        .await;
        current()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: result.ok().and_then(Result::ok),
        })
    }
}

#[cfg(test)]
mod tests;
