//! Official "听过的书" destination. This is independent of the syncable history timeline.
use super::super::{MusicClient, dto::AccountData, podcast};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::engine::Cancellation,
    podcast::{Episode, PodcastApi, Radio},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Mutex};

const PATH: &str = "/api/voice/book/recent";
const PAGE: usize = 20;
const MAX_BOOKS: usize = 1000;
const PROGRAM_PAGE: usize = 250;

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
    percent: Option<u8>,
    completed: bool,
    playable: bool,
}

#[derive(Clone)]
struct Chapter {
    id: String,
    name: String,
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
            name,
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
                let rows = self
                    .session
                    .run_for(&token, |api, _| async move {
                        decode(&api.call(PATH, json!({}), false, &[200]).await?)
                    })
                    .await?;
                self.session.with_token(&token, |_| {
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
        let (resources, selected) = self.session.run_for(token, |api, _| async {
            let mut voices = Vec::new();
            let mut seen = HashSet::new();
            // M/404 uses 250 per page and leaves asc absent without a saved order.
            for offset in (0..50_000).step_by(PROGRAM_PAGE) {
                if source.is_cancelled() {
                    return Err(Error::QueueChanged);
                }
                let response = api.call("/api/v6/dj/program/byradio",
                    json!({"radioId":radio.id,"offset":offset,"limit":PROGRAM_PAGE}), false, &[200]).await?;
                let data = &response["data"];
                let rows = data["programs"].as_array().filter(|rows| rows.len() <= PROGRAM_PAGE)
                    .ok_or(Error::Response("invalid listened book program page"))?;
                for raw in rows {
                    let mut voice = Episode::from_official(raw)?;
                    if !seen.insert(voice.id.clone()) {
                        return Err(Error::QueueChanged);
                    }
                    if voice.radio.is_none() {
                        voice.radio = Some(radio.clone());
                    }
                    voices.push(voice);
                }
                match data["more"].as_bool() {
                    Some(false) => break,
                    None if data["more"].is_null() => break,
                    Some(true) if !rows.is_empty() && offset + PROGRAM_PAGE < 50_000 => (),
                    _ => return Err(Error::Response("listened book program pages do not advance")),
                }
            }
            let mut resources = Vec::with_capacity(voices.len());
            let mut selected = None;
            let api = PodcastApi::new(api);
            for mut voice in voices {
                if source.is_cancelled() {
                    return Err(Error::QueueChanged);
                }
                let is_selected = voice.id == chapter.id;
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
                    // Progress only for this exact chapter, never an audio authorization.
                    resource["voice"]["djPlayRecordVo"] = json!({
                        "id":chapter.id, "listenLocation":chapter.location, "isListened":chapter.completed
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
