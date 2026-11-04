//! The ordinary PC Choice route (/choice, M/3145), distinct from Home and VIP.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    protocol::{
        api::{Api, identifier},
        artwork::{original, square},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

const PLAYLISTS: &str = "/api/playlist/category/list";
const SONGS: &str = "/api/personalized/newsong";
const BOOKS: &str = "/api/podcast/common/single/block/get";
const VOICES: &str = "/api/pc/voicelist/rcmd/list";
const CACHE: &str = "pc-choice-essential-v1";

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryEssentialContent {
    playlists: Vec<EssentialCard>,
    songs: Vec<EssentialCard>,
    books: Option<EssentialBooks>,
    voices: Vec<EssentialCard>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinaryEssentialView {
    content: OrdinaryEssentialContent,
    failed_blocks: usize,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EssentialBooks {
    title: String,
    cards: Vec<EssentialCard>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EssentialCard {
    id: String,
    title: String,
    subtitle: Option<String>,
    cover_url: Option<String>,
    radio_id: Option<String>,
    history: bool,
}

fn label(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    if value.is_empty()
        || value.len() > 768
        || value.contains("://")
        || value.chars().any(char::is_control)
    {
        return None;
    }
    Some(value.chars().take(140).collect())
}

fn id(value: &Value) -> Option<String> {
    identifier(value).ok().filter(|value| value.len() <= 32)
}

fn card(identity: &Value, title: &Value) -> Option<EssentialCard> {
    Some(EssentialCard {
        id: id(identity)?,
        title: label(title)?,
        subtitle: None,
        cover_url: None,
        radio_id: None,
        history: false,
    })
}

fn list(
    value: &Value,
    limit: usize,
    project: impl FnMut(&Value) -> Option<EssentialCard>,
) -> Result<Vec<EssentialCard>> {
    let rows = value
        .as_array()
        .ok_or(Error::Response("missing choice list"))?;
    let mut seen = std::collections::HashSet::new();
    Ok(rows
        .iter()
        .take(limit)
        .filter_map(project)
        .filter(|card| seen.insert(card.id.clone()))
        .collect())
}

fn playlists(value: &Value) -> Result<Vec<EssentialCard>> {
    list(&value["playlists"], 6, |raw| {
        let title = raw
            .get("name")
            .filter(|v| label(v).is_some())
            .unwrap_or(&raw["title"]);
        let mut card = card(&raw["id"], title)?;
        card.cover_url = ["coverImgUrl", "picUrl", "coverUrl"]
            .into_iter()
            .find_map(|key| square(&raw[key]));
        Some(card)
    })
}

fn songs(value: &Value) -> Result<Vec<EssentialCard>> {
    list(&value["result"], 12, |entry| {
        let song = &entry["song"];
        let mut card = card(&song["id"], &song["name"])?;
        let album = song.get("album").unwrap_or(&song["al"]);
        let artists = song.get("artists").unwrap_or(&song["ar"]);
        let names = artists
            .as_array()
            .into_iter()
            .flatten()
            .take(4)
            .filter_map(|row| label(&row["name"]))
            .collect::<Vec<_>>()
            .join(" / ");
        card.subtitle = (!names.is_empty()).then_some(names);
        card.cover_url = square(&album["picUrl"]);
        Some(card)
    })
}

fn voices(value: &Value) -> Result<Vec<EssentialCard>> {
    list(&value["data"]["recommendVoiceVOS"], 12, |entry| {
        let voice = &entry["djProgram"];
        let identity = ["id", "resourceId", "voiceId", "productId"]
            .into_iter()
            .find_map(|key| id(&voice[key]))?;
        let title = voice
            .get("name")
            .filter(|v| label(v).is_some())
            .unwrap_or(&voice["voiceName"]);
        let mut card = card(&json!(identity), title)?;
        card.cover_url = square(&voice["coverUrl"]);
        card.radio_id = radio_id(&voice["radio"]);
        card.subtitle = label(&voice["dj"]["brand"]).or_else(|| label(&voice["radio"]["name"]));
        Some(card)
    })
}

fn radio_id(value: &Value) -> Option<String> {
    ["id", "resourceId", "voiceListId", "productId"]
        .into_iter()
        .find_map(|key| id(&value[key]))
}

fn books(value: &Value) -> Result<Option<EssentialBooks>> {
    let blocks = value["data"]["blockVOS"]
        .as_array()
        .ok_or(Error::Response("missing choice audiobook blocks"))?;
    let Some(block) = blocks.first() else {
        return Ok(None);
    };
    let cards = list(&block["creatives"], 30, |row| {
        let ui = &row["uiElement"];
        let title = &ui["mainTitle"]["title"];
        let history = title == "听过的有声书";
        let radio = &row["creativeExtInfoVO"]["radio"];
        let mut card = if history {
            EssentialCard {
                id: "history".into(),
                title: label(title)?,
                subtitle: None,
                cover_url: None,
                radio_id: None,
                history: true,
            }
        } else {
            let identity = radio_id(radio).or_else(|| {
                (row["creativeType"] == "voiceList" && radio.is_null())
                    .then(|| id(&row["creativeId"]))
                    .flatten()
            })?;
            let mut card = card(&json!(identity), title)?;
            card.radio_id = Some(identity);
            card
        };
        card.cover_url = original(&ui["image"]["imageUrl"]).or_else(|| original(&radio["picUrl"]));
        card.subtitle = if history {
            let names = row["creativeExtInfoVO"]["djPrograms"]
                .as_array()
                .into_iter()
                .flatten()
                .take(3)
                .filter_map(|row| label(&row["radio"]["name"]))
                .collect::<Vec<_>>()
                .join(" / ");
            (!names.is_empty()).then_some(names)
        } else {
            label(&ui["subTitle"]["title"])
        };
        Some(card)
    })?;
    Ok(Some(EssentialBooks {
        title: label(&block["uiElement"]["mainTitle"]["title"])
            .unwrap_or_else(|| "精选有声书".into()),
        cards,
    }))
}

async fn get<T>(
    api: &dyn Api,
    path: &str,
    args: Value,
    project: fn(&Value) -> Result<T>,
) -> Result<T> {
    let response = api.call(path, args, false, &[200]).await?;
    project(&response)
}

async fn book_category(api: &dyn Api, category: u8) -> Result<Option<EssentialBooks>> {
    // Keep extInfo as the serialized JSON string used by M/3145 and M/15.lf.
    let details = if category == 0 {
        r#"{"position":"essential"}"#.into()
    } else {
        format!(
            r#"{{"interestTypes":[{{"resourceId":"type{category}","resourceType":"","pubTime":0}}],"position":"essential"}}"#
        )
    };
    get(
        api,
        BOOKS,
        json!({
            "pageCode":"PC_HOMEPAGE_PODCAST",
            "blockCode":"PC_HOMEPAGE_VOICEBOOK_RCMD",
            "extInfo": details
        }),
        books,
    )
    .await
}

fn fallback<T>(result: Result<T>, cached: T, failures: &mut usize) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(err)
            if err.is_service_code(301)
                || matches!(err, Error::StaleResponse | Error::CredentialChanged) =>
        {
            Err(err)
        }
        Err(_) => {
            *failures += 1;
            Ok(cached)
        }
    }
}

type EssentialResults = (
    Result<Vec<EssentialCard>>,
    Result<Vec<EssentialCard>>,
    Result<Option<EssentialBooks>>,
    Result<Vec<EssentialCard>>,
);

async fn fetch(api: Arc<dyn Api>, category: u8) -> EssentialResults {
    tokio::join!(
        get(
            api.as_ref(),
            PLAYLISTS,
            json!({"cat":"官方","limit":6,"newStyle":true}),
            playlists
        ),
        get(api.as_ref(), SONGS, json!({"limit":12,"areaId":0}), songs),
        book_category(api.as_ref(), category),
        get(api.as_ref(), VOICES, json!({"limit":12}), voices),
    )
}

fn category_key(category: u8) -> Result<String> {
    if category > 4 {
        return Err(Error::InvalidInput("invalid choice audiobook category"));
    }
    Ok(format!("books:{category}"))
}

impl MusicClient {
    pub async fn ordinary_essential(
        &self,
        epoch: &str,
        category: u8,
    ) -> Result<AccountData<OrdinaryEssentialView>> {
        let key = category_key(category)?;
        let token = self.playlist_token(epoch)?;
        let (playlists, songs, books, voices) = self
            .session
            .run_for(&token, |api, _| async move {
                let results = fetch(api, category).await;
                for err in [
                    results.0.as_ref().err(),
                    results.1.as_ref().err(),
                    results.2.as_ref().err(),
                    results.3.as_ref().err(),
                ]
                .into_iter()
                .flatten()
                {
                    if err.is_service_code(301) {
                        return Err(Error::service("choice", json!(301), Value::Null));
                    }
                }
                Ok(results)
            })
            .await?;
        let value = self.session.with_token(&token, |uid| {
            let store = self.session.persistent_store().ok();
            let cached = store
                .as_ref()
                .and_then(|store| {
                    store
                        .get(uid, CACHE, "common")
                        .ok()
                        .flatten()
                        .and_then(|raw| {
                            serde_json::from_value::<OrdinaryEssentialContent>(raw).ok()
                        })
                })
                .unwrap_or_default();
            let cached_books = store.as_ref().and_then(|store| {
                store
                    .get(uid, CACHE, &key)
                    .ok()
                    .flatten()
                    .and_then(|raw| serde_json::from_value::<EssentialBooks>(raw).ok())
            });
            let mut failed_blocks = 0;
            let content = OrdinaryEssentialContent {
                playlists: fallback(playlists, cached.playlists, &mut failed_blocks)?,
                songs: fallback(songs, cached.songs, &mut failed_blocks)?,
                books: fallback(books, cached_books, &mut failed_blocks)?,
                voices: fallback(voices, cached.voices, &mut failed_blocks)?,
            };
            if failed_blocks < 4
                && let Some(store) = store
            {
                // The persisted data is only a read fallback; an unavailable store
                // cannot turn a successful service response into a network error.
                let mut common = serde_json::to_value(&content)?;
                common["books"] = Value::Null;
                let _ = store.put(uid, CACHE, "common", &common);
                let _ = store.put(uid, CACHE, &key, &serde_json::to_value(&content.books)?);
            }
            Ok(OrdinaryEssentialView {
                content,
                failed_blocks,
            })
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn ordinary_essential_books(
        &self,
        epoch: &str,
        category: u8,
    ) -> Result<AccountData<Option<EssentialBooks>>> {
        let key = category_key(category)?;
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                book_category(api.as_ref(), category).await
            })
            .await?;
        self.session.with_token(&token, |uid| {
            if let (Some(books), Ok(store)) = (&value, self.session.persistent_store()) {
                let _ = store.put(uid, CACHE, &key, &serde_json::to_value(books)?);
            }
            Ok(())
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
