//! Official PC new song notice: passive indicator and sidebar visit are distinct.
use super::{AccountData, MusicClient};
use crate::{Error, Result, client::mapping};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const ENDPOINT: &str = "/api/user/friend/new/song/notice/web/get";
const RECORD: &str = "community-new-song-visit-v1";
const MAX_TIME: u64 = 8_640_000_000_000_000;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunityNoticeMode {
    Badge,
    Visit,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommunityNewSongNotice {
    show_dot: bool,
    card: Option<NoticeCard>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NoticeCard {
    style: NoticeStyle,
    text: String,
    avatars: Vec<String>,
    cover: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum NoticeStyle {
    Artists,
    Musicians,
}

fn stored_time(value: Option<Value>) -> u64 {
    value
        .and_then(|value| valid_time(&value))
        .unwrap_or_default()
}

fn valid_time(value: &Value) -> Option<u64> {
    value.as_u64().filter(|value| *value <= MAX_TIME)
}

fn now() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::State("invalid community notice clock"))?;
    u64::try_from(elapsed.as_millis())
        .ok()
        .filter(|value| *value <= MAX_TIME)
        .ok_or(Error::State("invalid community notice clock"))
}

fn card(data: &Value) -> Option<NoticeCard> {
    let style = match data["style"].as_str()? {
        "all-normal" => NoticeStyle::Artists,
        "musician-normal" => NoticeStyle::Musicians,
        _ => return None,
    };
    let text = data["text"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect();
    let artwork = |value: &Value| mapping::artwork(&value["url"]);
    Some(NoticeCard {
        style,
        text,
        avatars: data["leftIcons"]
            .as_array()
            .into_iter()
            .flatten()
            .take(2)
            .filter_map(artwork)
            .collect(),
        cover: data["rightIcons"].get(0).and_then(artwork),
    })
}

impl MusicClient {
    pub async fn community_new_song_notice(
        &self,
        epoch: &str,
        mode: CommunityNoticeMode,
    ) -> Result<AccountData<CommunityNewSongNotice>> {
        let token = self.playlist_token(epoch)?;
        let store = self.session.persistent_store()?;
        let args = match mode {
            CommunityNoticeMode::Badge => json!({}),
            CommunityNoticeMode::Visit => self.session.with_token(&token, |uid| {
                let previous = stored_time(store.get(uid, RECORD, "last_view_ms")?);
                Ok(if previous == 0 {
                    json!({})
                } else {
                    json!({"time": previous})
                })
            })?,
        };
        let response = self
            .session
            .run_for(&token, |api, _| async move {
                api.call(ENDPOINT, args, false, &[200]).await
            })
            .await?;
        let data = &response["data"];
        self.session.with_token(&token, |uid| {
            let value = match mode {
                CommunityNoticeMode::Badge => {
                    // A late badge response observes the newest successfully
                    // recorded visit, not the value from before its request.
                    let viewed = stored_time(store.get(uid, RECORD, "last_view_ms")?);
                    CommunityNewSongNotice {
                        show_dot: data["style"] != "musician-empty"
                            && valid_time(&data["time"]).is_some_and(|time| time > viewed),
                        card: None,
                    }
                }
                CommunityNoticeMode::Visit => {
                    store.transaction(|tx| {
                        let viewed = stored_time(tx.get(uid, RECORD, "last_view_ms")?);
                        tx.put(uid, RECORD, "last_view_ms", &json!(viewed.max(now()?)))
                    })?;
                    CommunityNewSongNotice {
                        show_dot: false,
                        card: card(data),
                    }
                }
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
