//! M/15.Uh and M/2264: listening rankings are distinct from recent history.
use super::{MusicClient, dto::AccountData, dto::TrackView, mapping};
use crate::{
    Error, Result,
    error::code_matches,
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const LIMIT: usize = 120;
const MAX_UI_COUNT: u64 = 9_007_199_254_740_991;
const RECORD: &str = "/api/v1/play/record";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfilePlayRecordPeriod {
    Week,
    All,
}
impl ProfilePlayRecordPeriod {
    fn fields(self) -> (u8, &'static str) {
        match self {
            Self::Week => (1, "weekData"),
            Self::All => (0, "allData"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePlayRecordEntry {
    pub track: TrackView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_count: Option<u64>,
}

#[derive(Serialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProfilePlayRecordView {
    Ready {
        is_self: bool,
        items: Vec<ProfilePlayRecordEntry>,
    },
    Denied,
}

pub(super) fn count(value: &Value) -> Option<u64> {
    value.as_u64().filter(|v| *v <= MAX_UI_COUNT)
}

pub(super) fn allowed(detail: &Value, is_self: bool) -> bool {
    is_self
        || (detail["peopleCanSeeMyPlayRecord"].as_bool() == Some(true)
            && detail["profile"]["inBlacklist"].as_bool() != Some(true))
}

fn denied(error: &Error) -> bool {
    error.is_service_code(403)
        || error.is_service_code(404)
        || matches!(
            error.root(),
            Error::HttpStatus {
                status: 403 | 404,
                ..
            }
        )
}

async fn read(
    api: &dyn Api,
    uid: &str,
    period: ProfilePlayRecordPeriod,
    is_self: bool,
) -> Result<ProfilePlayRecordView> {
    let (kind, field) = period.fields();
    let response = match api
        .call(
            RECORD,
            json!({"offset":0,"total":true,"limit":LIMIT,"uid":uid,"type":kind}),
            false,
            &[200],
        )
        .await
    {
        Err(error) if denied(&error) => return Ok(ProfilePlayRecordView::Denied),
        other => other?,
    };
    let rows = response[field]
        .as_array()
        .filter(|rows| rows.len() <= LIMIT)
        .ok_or(Error::Response("invalid profile play record list"))?;
    let items = rows
        .iter()
        .map(|row| {
            let song = &row["song"];
            let mut track = mapping::track(song)?;
            track.voice_id = None;
            Ok(ProfilePlayRecordEntry {
                track,
                play_count: is_self.then(|| count(&row["playCount"]).unwrap_or_default()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ProfilePlayRecordView::Ready { is_self, items })
}

impl MusicClient {
    pub async fn profile_play_record(
        &self,
        epoch: &str,
        uid: Option<&str>,
        period: ProfilePlayRecordPeriod,
    ) -> Result<AccountData<ProfilePlayRecordView>> {
        let token = self.playlist_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, viewer| {
                let token = &token;
                async move {
                    let viewer = identifier(&json!(viewer))
                        .map_err(|_| Error::State("profile play records require an account"))?;
                    let uid = uid
                        .map(|id| identifier(&json!(id)))
                        .transpose()?
                        .unwrap_or_else(|| viewer.clone());
                    if uid.len() > 64 {
                        return Err(Error::InvalidInput("invalid profile identifier length"));
                    }
                    let is_self = viewer == uid;
                    if !is_self {
                        let path = format!("/api/w/v1/user/detail/{uid}");
                        let response = match api
                            .call(&path, json!({"all":true,"userId":uid}), false, &[200, 404])
                            .await
                        {
                            Ok(value) if code_matches(&value["code"], 404) => {
                                return Ok(ProfilePlayRecordView::Denied);
                            }
                            Err(error) if denied(&error) => {
                                return Ok(ProfilePlayRecordView::Denied);
                            }
                            other => other?,
                        };
                        if identifier(&response["profile"]["userId"]).ok().as_deref()
                            != Some(uid.as_str())
                        {
                            return Err(Error::Response("profile identity mismatch"));
                        }
                        if !allowed(&response, false) {
                            return Ok(ProfilePlayRecordView::Denied);
                        }
                        self.session.require_api(token, &api)?;
                    }
                    read(api.as_ref(), &uid, period, is_self).await
                }
            })
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
