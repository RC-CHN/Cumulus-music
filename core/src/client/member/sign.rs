//! Read-only membership check-in card and recorded results (M/15, M/154, M/208, M/616).
use super::{MusicClient, mapping};
use crate::{Error, Result, client::dto::AccountData, protocol::api::identifier};
use serde::Serialize;
use serde_json::{Value, json};

const CARD: &str = "/api/vipnewcenter/app/minidesk/music/sign/pc";
const HISTORY: &str = "/api/vipnewcenter/app/level/user/checkin/history/list";
const DETAIL: &str = "/api/vipnewcenter/app/level/user/checkin/history/detail";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberSignCard {
    title: String,
    subtitle: String,
    button_text: Option<String>,
    texture_url: Option<String>,
    days: Vec<SignDay>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignDay {
    label: String,
    today: bool,
    signed: bool,
    sign_time: Option<u64>,
    cover_url: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberSignDetail {
    time: u64,
    song_id: String,
    song: String,
    artist: Option<String>,
    cover_url: Option<String>,
    wish: Option<String>,
    wish_author: Option<String>,
    month_total: Option<u64>,
    next_reward: Option<SignReward>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignReward {
    days_to_go: u64,
    title: String,
    subtitle: Option<String>,
}

fn text(value: &Value, limit: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    (!value.is_empty()).then(|| value.chars().take(limit).collect())
}

fn unsigned(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn timestamp(value: &Value) -> Option<u64> {
    unsigned(value).filter(|v| (1..=8_640_000_000_000_000).contains(v))
}

fn card(data: &Value) -> Result<MemberSignCard> {
    let days = data["signInfoList"]
        .as_array()
        .filter(|days| days.len() <= 31)
        .ok_or(Error::Response("invalid sign card days"))?
        .iter()
        .map(|day| {
            let signed = day["sign"]
                .as_bool()
                .ok_or(Error::Response("invalid sign card state"))?;
            Ok(SignDay {
                label: text(&day["dayText"], 32)
                    .ok_or(Error::Response("invalid sign card date"))?,
                today: day["today"].as_bool().unwrap_or(false),
                signed,
                sign_time: timestamp(&day["signTime"]).filter(|_| signed),
                cover_url: signed
                    .then(|| mapping::artwork(&day["songCoverUrl"]))
                    .flatten(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(MemberSignCard {
        title: text(&data["text"], 80).unwrap_or_else(|| "黑胶乐签".into()),
        subtitle: text(&data["subText"], 200).unwrap_or_default(),
        button_text: text(&data["btnText"], 32),
        texture_url: crate::protocol::artwork::original(&data["bgTexture"]),
        days,
    })
}

fn record_for(data: &Value, sign_time: u64) -> Result<Value> {
    let record = data
        .as_array()
        .filter(|list| list.len() <= 3660)
        .ok_or(Error::Response("invalid sign history list"))?
        .iter()
        .find(|record| timestamp(&record["dayTime"]) == Some(sign_time))
        .ok_or(Error::InvalidInput("sign date not in history"))?;
    let id = &record["recordId"];
    identifier(id).map_err(|_| Error::Response("invalid sign history record"))?;
    Ok(id.clone())
}

fn detail(data: &Value, sign_time: u64) -> Result<MemberSignDetail> {
    let song = &data["songInfo"];
    let month_total = unsigned(&data["monthCheckInTotalDay"]).filter(|count| *count <= 31);
    let next_reward = month_total.and_then(|total| {
        let rewards = data["monthCheckInPrizList"].as_array()?;
        if rewards.len() > 31 {
            return None;
        }
        rewards
            .iter()
            .filter_map(|reward| {
                let day = unsigned(&reward["day"]).filter(|day| *day > total && *day <= 31)?;
                Some(SignReward {
                    days_to_go: day - total,
                    title: text(&reward["prizeShowName"], 80)?,
                    subtitle: text(&reward["showSubTitle"], 80),
                })
            })
            .min_by_key(|reward| reward.days_to_go)
    });
    Ok(MemberSignDetail {
        time: timestamp(&data["time"]).unwrap_or(sign_time),
        song_id: identifier(&song["songId"]).map_err(|_| Error::Response("invalid sign song"))?,
        song: text(&song["songName"], 300).ok_or(Error::Response("invalid sign song name"))?,
        artist: text(&song["artistName"], 200),
        cover_url: mapping::artwork(&song["cover"]),
        wish: text(&data["wishWords"], 1000),
        wish_author: text(&data["wishUserNickname"], 200),
        month_total,
        next_reward,
    })
}

impl MusicClient {
    pub async fn member_sign_card(&self, epoch: &str) -> Result<AccountData<MemberSignCard>> {
        let token = self.member_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let response = api.call(CARD, json!({"type":1}), false, &[200]).await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: card(&response["data"])?,
            })
        })
    }

    pub async fn member_sign_detail(
        &self,
        epoch: &str,
        sign_time: u64,
    ) -> Result<AccountData<MemberSignDetail>> {
        if timestamp(&json!(sign_time)).is_none() {
            return Err(Error::InvalidInput("invalid sign date"));
        }
        let token = self.member_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let records = api.call(HISTORY, json!({}), false, &[200]).await?;
        let record_id = self
            .session
            .with_token(&token, |_| record_for(&records["data"], sign_time))?;
        // Do not fall back to the type=1 (new sign) result path. Only an actual
        // record from the currently authenticated account can supply an ID.
        let response = api
            .call(
                DETAIL,
                json!({"type":2,"recordId":record_id}),
                false,
                &[200],
            )
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: detail(&response["data"], sign_time)?,
            })
        })
    }
}
