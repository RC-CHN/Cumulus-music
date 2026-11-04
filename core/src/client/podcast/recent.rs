//! M3150/M15 recent updates. The wire cursor and ordered voice source stay native.
use super::{EpisodeView, MusicClient, PodcastApi, view};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::AccountData,
    playback::engine::Cancellation,
    podcast::Episode,
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Mutex};

const ENDPOINT: &str = "/api/social/my/recent/update/voice/list/v1";
const MAX_RESPONSE: usize = 200;
const MAX_ROWS: usize = 500;
const MAX_PAGES: usize = 20;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PodcastRecentQuery {
    Open,
    Refresh,
    Next { revision: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastRecentPage {
    revision: String,
    episodes: Vec<RecentEpisode>,
    has_more: bool,
    capped: bool,
    new_voice_count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecentEpisode {
    #[serde(flatten)]
    voice: EpisodeView,
    listener_count: Option<u64>,
    listen_location_ms: Option<u64>,
    listened: bool,
}

fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
}

fn unavailable(voice: &Episode) -> bool {
    voice.raw["grayDisplay"] == true
        || voice.raw["disPlayStatus"]
            .as_str()
            .is_some_and(|status| status != "NORMAL")
}

fn project(voice: &Episode) -> RecentEpisode {
    let raw = &voice.raw;
    let record = &raw["djPlayRecordVo"];
    let mut item = view::episode(voice, &[]);
    item.liked = voice.liked.unwrap_or(false);
    item.unavailable = unavailable(voice);
    RecentEpisode {
        voice: item,
        listener_count: ["listenerCount", "playCount", "adjustedPlayCount"]
            .iter()
            .filter_map(|key| number(&raw[key]))
            .find(|count| *count > 0)
            .or_else(|| number(&raw["listenerCount"])),
        listen_location_ms: number(&record["listenLocation"]).filter(|location| *location > 0),
        listened: record["isListened"] == true,
    }
}

struct RemotePage {
    rows: Vec<Episode>,
    more: bool,
    next: Value,
    new_voice_count: u64,
}

fn decode(value: &Value) -> Result<RemotePage> {
    let data = value["data"]
        .as_object()
        .ok_or(Error::Response("recent podcast response missing data"))?;
    let rows = match data.get("data") {
        Some(Value::Array(rows)) if rows.len() <= MAX_RESPONSE => rows
            .iter()
            .map(Episode::from_official)
            .collect::<Result<Vec<_>>>()?,
        None | Some(Value::Null) => Vec::new(),
        _ => {
            return Err(Error::Response(
                "recent podcast response exceeds page bounds",
            ));
        }
    };
    Ok(RemotePage {
        rows,
        more: data.get("hasMore").is_some_and(js_truthy),
        next: data.get("updateTime").cloned().unwrap_or(Value::Null),
        new_voice_count: data.get("newVoiceCount").and_then(number).unwrap_or(0),
    })
}

struct Snapshot {
    revision: String,
    rows: Vec<Episode>,
    cursor: Option<Value>,
    seen_cursors: HashSet<u64>,
    pages: usize,
    capped: bool,
    new_voice_count: u64,
    source: Cancellation,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: uuid::Uuid::new_v4().simple().to_string(),
            rows: Vec::new(),
            cursor: None,
            seen_cursors: HashSet::new(),
            pages: 0,
            capped: false,
            new_voice_count: 0,
            source: Cancellation::default(),
        }
    }
}

impl Snapshot {
    fn view(&self) -> PodcastRecentPage {
        PodcastRecentPage {
            revision: self.revision.clone(),
            episodes: self.rows.iter().map(project).collect(),
            has_more: self.cursor.is_some(),
            capped: self.capped,
            new_voice_count: self.new_voice_count,
        }
    }

    fn append(&mut self, page: RemotePage) -> Result<()> {
        let cursor_time = if page.more {
            Some(
                number(&page.next)
                    .filter(|time| *time > 0 && !self.seen_cursors.contains(time))
                    .ok_or(Error::Response("recent podcast cursor does not advance"))?,
            )
        } else {
            None
        };
        let mut ids: HashSet<String> = self.rows.iter().map(|row| row.id.clone()).collect();
        let mut rows = page
            .rows
            .into_iter()
            .filter(|row| ids.insert(row.id.clone()));
        self.rows
            .extend(rows.by_ref().take(MAX_ROWS - self.rows.len()));
        if self.pages == 0 {
            self.new_voice_count = page.new_voice_count;
        }
        self.pages += 1;
        self.capped = rows.next().is_some()
            || (page.more && (self.rows.len() == MAX_ROWS || self.pages == MAX_PAGES));
        self.cursor = cursor_time.filter(|_| !self.capped).map(|time| {
            self.seen_cursors.insert(time);
            page.next
        });
        self.source.cancel();
        self.source = Cancellation::default();
        self.revision = uuid::Uuid::new_v4().simple().to_string();
        Ok(())
    }
}

#[derive(Default)]
struct Slot {
    token: Option<SessionToken>,
    snapshot: Option<Snapshot>,
}

#[derive(Default)]
pub(in crate::client) struct PodcastRecentHost {
    slot: Mutex<Slot>,
    load: tokio::sync::Mutex<()>,
}

impl PodcastRecentHost {
    fn with_slot<T>(
        &self,
        token: &SessionToken,
        f: impl FnOnce(&mut Slot) -> Result<T>,
    ) -> Result<T> {
        let mut slot = self.slot.lock().map_err(|_| Error::LockPoisoned)?;
        if slot.token.as_ref() != Some(token) {
            if let Some(old) = slot.snapshot.take() {
                old.source.cancel();
            }
            slot.token = Some(token.clone());
        }
        f(&mut slot)
    }
}

pub(in crate::client) struct RecentPlayback {
    pub resources: Vec<Value>,
    pub index: usize,
    pub source: Cancellation,
}

impl MusicClient {
    pub async fn podcast_recent(
        &self,
        epoch: &str,
        query: PodcastRecentQuery,
    ) -> Result<AccountData<PodcastRecentPage>> {
        let token = self.podcast_token(epoch)?;
        let _load = self
            .podcast_recent
            .load
            .try_lock()
            .map_err(|_| Error::MutationBusy)?;
        let result = |value| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        };
        let (current, cursor) = self.session.with_token(&token, |_| {
            self.podcast_recent.with_slot(&token, |slot| match &query {
                PodcastRecentQuery::Open => Ok((slot.snapshot.as_ref().map(Snapshot::view), None)),
                PodcastRecentQuery::Refresh => Ok((None, None)),
                PodcastRecentQuery::Next { revision } => {
                    let snapshot = slot
                        .snapshot
                        .as_ref()
                        .filter(|snapshot| &snapshot.revision == revision)
                        .ok_or(Error::QueueChanged)?;
                    Ok((
                        None,
                        Some(snapshot.cursor.clone().ok_or(Error::QueueChanged)?),
                    ))
                }
            })
        })?;
        if let Some(current) = current {
            return result(current);
        }
        let append = cursor.is_some();
        let page = self
            .session
            .run_for(&token, |api, _| async move {
                // M3150 passes UID to the formatter, not to the endpoint. No limit or offset.
                let body = cursor.map_or_else(|| json!({}), |time| json!({"updateTime":time}));
                decode(&api.call(ENDPOINT, body, false, &[200]).await?)
            })
            .await?;
        self.session.with_token(&token, |_| {
            self.podcast_recent.with_slot(&token, |slot| {
                if append {
                    let snapshot = slot.snapshot.as_mut().ok_or(Error::QueueChanged)?;
                    snapshot.append(page)?;
                    result(snapshot.view())
                } else {
                    let mut snapshot = Snapshot::default();
                    snapshot.append(page)?;
                    let view = snapshot.view();
                    if let Some(old) = slot.snapshot.replace(snapshot) {
                        old.source.cancel();
                    }
                    result(view)
                }
            })
        })
    }

    pub(in crate::client) async fn podcast_recent_resources(
        &self,
        token: &SessionToken,
        revision: &str,
        id: &str,
        full_list: bool,
    ) -> Result<RecentPlayback> {
        let id = identifier(&json!(id))?;
        let (rows, source) = self.session.with_token(token, |_| {
            self.podcast_recent.with_slot(token, |slot| {
                let snapshot = slot
                    .snapshot
                    .as_ref()
                    .filter(|snapshot| snapshot.revision == revision)
                    .ok_or(Error::QueueChanged)?;
                let selected = snapshot
                    .rows
                    .iter()
                    .find(|row| row.id == id)
                    .ok_or(Error::QueueChanged)?;
                if unavailable(selected) {
                    return Err(Error::State("program is unavailable"));
                }
                let rows = if full_list {
                    snapshot.rows.clone()
                } else {
                    vec![selected.clone()]
                };
                Ok((rows, snapshot.source.clone()))
            })
        })?;
        let mut resources = Vec::with_capacity(rows.len());
        let mut index = None;
        for mut row in rows {
            if source.is_cancelled() {
                return Err(Error::QueueChanged);
            }
            let selected = row.id == id;
            if unavailable(&row) {
                continue;
            }
            if row.song_id.is_none() {
                // M3085 hydrates the clicked resource. Keep the selection
                // responsive if another table row lacks its media identity.
                if !selected {
                    continue;
                }
                row = self
                    .session
                    .run_for(token, |api, _| async {
                        PodcastApi::new(api).episode(&json!(row.id)).await
                    })
                    .await?;
                if unavailable(&row) {
                    return Err(Error::State("program is unavailable"));
                }
            }
            if selected {
                index = Some(resources.len());
            }
            let mut resource = row.playback_resource()?;
            resource["song"] = row.download_metadata()?;
            resource["level"] = json!("standard");
            resources.push(resource);
        }
        self.session.check_generation(token)?;
        if source.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        Ok(RecentPlayback {
            resources,
            index: index.ok_or(Error::QueueChanged)?,
            source,
        })
    }
}
