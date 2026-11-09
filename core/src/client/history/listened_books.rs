//! Official "听过的书" destination. This is independent of the syncable history timeline.
use super::super::{
    MusicClient,
    dto::AccountData,
    podcast::{
        self,
        radio_playback::{around_request, around_response, by_radio},
    },
};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::engine::Cancellation,
    podcast::{Episode, PodcastApi, Radio, progress::latest, radio_order},
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;

const PATH: &str = "/api/voice/book/recent";
const PAGE: usize = 20;
const MAX_BOOKS: usize = 1000;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenedBooksQuery {
    Open,
    Page { revision: String, number: usize },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenedBooksPage {
    revision: String,
    number: usize,
    pages: usize,
    total: usize,
    rows: Vec<BookRow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BookRow {
    key: usize,
    radio: podcast::PodcastCard,
    subscribers: Option<u64>,
    chapter: Option<ChapterView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChapterView {
    name: String,
    recent: bool,
    percent: Option<u8>,
    completed: bool,
    playable: bool,
}

#[derive(Clone)]
struct Chapter {
    id: String,
    media_id: Option<String>,
    name: String,
    recent: bool,
    location: u64,
    duration: u64,
    completed: bool,
    playable: bool,
}

struct Book {
    radio: Radio,
    chapter: Option<Chapter>,
}

fn number_value(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn unavailable(value: &Value) -> bool {
    value["grayDisplay"] == true
        || value["underShelf"] == true
        || value["disPlayStatus"]
            .as_str()
            .is_some_and(|status| status != "NORMAL")
}

fn book(value: &Value) -> Result<Book> {
    let radio = Radio::from_official(&value["radio"])?;
    let episode = Episode::from_official(value).ok();
    let record = &value["djPlayRecordVo"];
    let chapter = if let Some(episode) = episode {
        let name = if episode.name.is_empty() {
            radio.raw["voiceName"].as_str().unwrap_or("").to_owned()
        } else {
            episode.name
        };
        let record_matches =
            record["id"].is_null() || identifier(&record["id"]).is_ok_and(|id| id == episode.id);
        (!name.is_empty()).then(|| Chapter {
            id: episode.id,
            media_id: episode.song_id,
            name,
            recent: false,
            location: if record_matches {
                number_value(&record["listenLocation"]).unwrap_or(0)
            } else {
                0
            },
            duration: episode.duration_ms,
            completed: record_matches && record["isListened"] == true,
            playable: !unavailable(&episode.raw),
        })
    } else {
        None
    };
    Ok(Book { radio, chapter })
}

fn decode(value: &Value) -> Result<Vec<Book>> {
    let data = value["data"]
        .as_object()
        .ok_or(Error::Response("missing listened books"))?;
    let mut rows = Vec::new();
    for key in ["currentList", "yesterdayList", "passingList"] {
        match data.get(key) {
            None | Some(Value::Null) => (),
            Some(Value::Array(group)) if rows.len() + group.len() <= MAX_BOOKS => {
                rows.extend(group.iter().map(book).collect::<Result<Vec<_>>>()?);
            }
            _ => return Err(Error::Response("invalid listened books group")),
        }
    }
    Ok(rows)
}

fn local_chapter(chapter: &mut Chapter, local: latest::LatestVoice) {
    if local.id != chapter.id
        || local.name.is_empty()
        || chapter
            .media_id
            .as_ref()
            .is_some_and(|id| id != &local.main_track_id)
    {
        return;
    }
    chapter.recent = true;
    chapter.name = local.name;
    chapter.media_id = Some(local.main_track_id);
    chapter.location = local.time * 1000;
    chapter.duration = local.duration * 1000;
    chapter.completed = local.time > 0 && local.time >= local.duration;
}

async fn around(api: &dyn Api, chapter: &str, ascending: bool) -> Result<Vec<Episode>> {
    around_response(&around_request(api, chapter).await?, ascending)?
        .ok_or(Error::Response("invalid around-program list"))
}

struct Snapshot {
    token: SessionToken,
    revision: String,
    rows: Vec<Book>,
    source: Cancellation,
}

impl Snapshot {
    fn page(&self, number: usize) -> Result<ListenedBooksPage> {
        if number == 0 || number > self.rows.len().div_ceil(PAGE).max(1) {
            return Err(Error::InvalidInput("invalid listened books page"));
        }
        let offset = (number - 1) * PAGE;
        let rows = self
            .rows
            .iter()
            .enumerate()
            .skip(offset)
            .take(PAGE)
            .map(|(key, row)| {
                let radio = podcast::view::card(&row.radio);
                BookRow {
                    key,
                    subscribers: number_value(&row.radio.raw["subCount"]),
                    chapter: row.chapter.as_ref().map(|chapter| ChapterView {
                        name: chapter.name.clone(),
                        recent: chapter.recent,
                        percent: if chapter.location > 0
                            && chapter.duration > 0
                            && !chapter.completed
                        {
                            Some(
                                (u128::from(chapter.location) * 100 / u128::from(chapter.duration))
                                    .clamp(1, 100) as u8,
                            )
                        } else {
                            None
                        },
                        completed: chapter.completed,
                        playable: chapter.playable && !unavailable(&row.radio.raw),
                    }),
                    radio,
                }
            })
            .collect();
        Ok(ListenedBooksPage {
            revision: self.revision.clone(),
            number,
            pages: self.rows.len().div_ceil(PAGE),
            total: self.rows.len(),
            rows,
        })
    }
}

#[derive(Default)]
pub(in crate::client) struct ListenedBooksHost {
    slot: Mutex<Option<Snapshot>>,
    loading: tokio::sync::Mutex<()>,
}

impl ListenedBooksHost {
    fn current<T>(
        &self,
        token: &SessionToken,
        revision: &str,
        f: impl FnOnce(&Snapshot) -> Result<T>,
    ) -> Result<T> {
        let slot = self.slot.lock().map_err(|_| Error::LockPoisoned)?;
        let snapshot = slot
            .as_ref()
            .filter(|snapshot| &snapshot.token == token && snapshot.revision == revision)
            .ok_or(Error::QueueChanged)?;
        f(snapshot)
    }
}

impl MusicClient {
    pub async fn listened_books(
        &self,
        epoch: &str,
        query: ListenedBooksQuery,
    ) -> Result<AccountData<ListenedBooksPage>> {
        let token = self.player_token(epoch)?;
        let value = match query {
            ListenedBooksQuery::Open => {
                let _reading = self
                    .listened_books
                    .loading
                    .try_lock()
                    .map_err(|_| Error::MutationBusy)?;
                let mut rows = self
                    .session
                    .run_for(&token, |api, _| async move {
                        decode(&api.call(PATH, json!({}), false, &[200]).await?)
                    })
                    .await?;
                self.session.with_token(&token, |owner| {
                    let store = self.session.persistent_store()?;
                    store.read_snapshot(|tx| {
                        for row in &mut rows {
                            if let Some(chapter) = row.chapter.as_mut()
                                && let Some(local) = latest::read(tx, owner, &row.radio.id)?
                            {
                                local_chapter(chapter, local);
                            }
                        }
                        Ok(())
                    })?;
                    let snapshot = Snapshot {
                        token: token.clone(),
                        revision: uuid::Uuid::new_v4().simple().to_string(),
                        rows,
                        source: Cancellation::default(),
                    };
                    let page = snapshot.page(1)?;
                    let mut slot = self
                        .listened_books
                        .slot
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?;
                    if let Some(old) = slot.replace(snapshot) {
                        old.source.cancel();
                    }
                    Ok(page)
                })?
            }
            ListenedBooksQuery::Page { revision, number } => {
                self.session.with_token(&token, |_| {
                    self.listened_books
                        .current(&token, &revision, |snapshot| snapshot.page(number))
                })?
            }
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    fn with_listened_chapter<T>(
        &self,
        token: &SessionToken,
        revision: &str,
        key: usize,
        f: impl FnOnce(&Radio, &Chapter, &Cancellation) -> T,
    ) -> Result<T> {
        self.session.with_token(token, |_| {
            self.listened_books.current(token, revision, |snapshot| {
                let book = snapshot.rows.get(key).ok_or(Error::QueueChanged)?;
                let chapter = book
                    .chapter
                    .as_ref()
                    .filter(|chapter| chapter.playable)
                    .ok_or(Error::State("listened chapter unavailable"))?;
                if unavailable(&book.radio.raw) {
                    return Err(Error::State("listened book unavailable"));
                }
                Ok(f(&book.radio, chapter, &snapshot.source))
            })
        })
    }

    pub(in crate::client) fn check_listened_chapter(
        &self,
        token: &SessionToken,
        revision: &str,
        key: usize,
    ) -> Result<()> {
        self.with_listened_chapter(token, revision, key, |_, _, _| ())
    }

    pub(in crate::client) async fn listened_book_queue(
        &self,
        token: &SessionToken,
        revision: &str,
        key: usize,
    ) -> Result<(Vec<Value>, usize, Value, Cancellation)> {
        let (radio, chapter, source) =
            self.with_listened_chapter(token, revision, key, |radio, chapter, source| {
                (radio.clone(), chapter.clone(), source.clone())
            })?;
        let (locally_latest, ascending) = self.session.with_token(token, |owner| {
            let store = self.session.persistent_store()?;
            store.read_snapshot(|tx| {
                Ok((
                    latest::read(tx, owner, &radio.id)?.is_some_and(|value| value.id == chapter.id),
                    radio_order::read(tx, owner, &radio.id)?,
                ))
            })
        })?;
        let (resources, selected) = self.session.run_for(token, |api, _| async {
            if source.is_cancelled() {
                return Err(Error::QueueChanged);
            }
            // M/404 rechecks the book's latest record when resolving the
            // clicked chapter. It must never change the chapter selected.
            let voices = if locally_latest {
                around(api.as_ref(), &chapter.id, ascending.unwrap_or(false)).await?
            } else {
                by_radio(api.as_ref(), &radio.id, ascending, &source).await?
            };
            let mut resources = Vec::with_capacity(voices.len());
            let mut selected = None;
            let api = PodcastApi::new(api);
            for mut voice in voices {
                if source.is_cancelled() {
                    return Err(Error::QueueChanged);
                }
                let is_selected = voice.id == chapter.id;
                if voice.radio.is_none() {
                    voice.radio = Some(radio.clone());
                }
                if voice.song_id.is_none() {
                    if !is_selected {
                        continue;
                    }
                    voice = api.episode(&json!(voice.id)).await?;
                    if voice.radio.is_none() {
                        voice.radio = Some(radio.clone());
                    }
                }
                if unavailable(&voice.raw) {
                    continue;
                }
                let mut resource = voice.playback_resource()?;
                if is_selected {
                    if voice.radio.as_ref().is_none_or(|item| item.id != radio.id) {
                        return Err(Error::QueueChanged);
                    }
                    selected = Some(resources.len());
                    // This is the clicked row's explicit breakpoint, with no
                    // retained offset if fresh metadata names a different media.
                    let media_matches = chapter.media_id.as_deref()
                        .is_none_or(|media| voice.song_id.as_deref() == Some(media));
                    let position = if media_matches && !chapter.completed && chapter.location < chapter.duration {
                        chapter.location
                    } else { 0 };
                    resource["voice"]["djPlayRecordVo"] = json!({
                        "id":chapter.id, "listenLocation":position, "isListened":chapter.completed
                    });
                }
                resource["song"] = voice.download_metadata()?;
                resource["level"] = json!("standard");
                resources.push(resource);
            }
            Ok((resources, selected.ok_or(Error::QueueChanged)?))
        }).await?;
        if source.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        Ok((
            resources,
            selected,
            json!({"kind":"listened_book","scene":"unknown","id":radio.id,"text":radio.name}),
            source,
        ))
    }
}
