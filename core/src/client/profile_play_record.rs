//! M/15.Uh and M/2264: listening rankings are distinct from recent history.
mod play;
mod removal;
use super::{MusicClient, dto::AccountData, dto::TrackView, mapping};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    error::code_matches,
    protocol::api::{Api, identifier},
};
pub(super) use removal::RemovalHost;
pub use removal::{ProfilePlayRecordRemovalAction, ProfilePlayRecordRemovalOutcome};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub removal_uncertain: bool,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
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

fn target(
    session: &AccountSession,
    token: &SessionToken,
    uid: Option<&str>,
) -> Result<(String, bool)> {
    let viewer = session.with_token(token, |uid| identifier(&json!(uid)))?;
    let uid = uid
        .map(|id| identifier(&json!(id)))
        .transpose()?
        .unwrap_or_else(|| viewer.clone());
    if uid.len() > 64 {
        return Err(Error::InvalidInput("invalid profile identifier length"));
    }
    let own = viewer == uid;
    Ok((uid, own))
}

async fn visible(
    session: &AccountSession,
    token: &SessionToken,
    api: &Arc<dyn Api>,
    uid: &str,
) -> Result<bool> {
    let path = format!("/api/w/v1/user/detail/{uid}");
    let response = match api
        .call(&path, json!({"all":true,"userId":uid}), false, &[200, 404])
        .await
    {
        Ok(value) if code_matches(&value["code"], 404) => return Ok(false),
        Err(error) if denied(&error) => return Ok(false),
        other => other?,
    };
    if identifier(&response["profile"]["userId"]).ok().as_deref() != Some(uid) {
        return Err(Error::Response("profile identity mismatch"));
    }
    if !allowed(&response, false) {
        return Ok(false);
    }
    session.require_api(token, api)?;
    Ok(true)
}

async fn rank_rows(
    api: &dyn Api,
    uid: &str,
    period: ProfilePlayRecordPeriod,
) -> Result<Option<Vec<Value>>> {
    let (kind, field) = period.fields();
    let mut response = match api
        .call(
            RECORD,
            json!({"offset":0,"total":true,"limit":LIMIT,"uid":uid,"type":kind}),
            false,
            &[200],
        )
        .await
    {
        Err(error) if denied(&error) => return Ok(None),
        other => other?,
    };
    let Value::Array(rows) = response[field].take() else {
        return Err(Error::Response("invalid profile play record list"));
    };
    if rows.len() > LIMIT {
        return Err(Error::Response("invalid profile play record list"));
    }
    Ok(Some(rows))
}

async fn read(
    api: &dyn Api,
    uid: &str,
    period: ProfilePlayRecordPeriod,
    is_self: bool,
) -> Result<ProfilePlayRecordView> {
    let Some(rows) = rank_rows(api, uid, period).await? else {
        return Ok(ProfilePlayRecordView::Denied);
    };
    let items = rows
        .iter()
        .map(|row| {
            let song = &row["song"];
            let mut track = mapping::track(song)?;
            track.voice_id = None;
            Ok(ProfilePlayRecordEntry {
                track,
                play_count: is_self.then(|| count(&row["playCount"]).unwrap_or_default()),
                removal_uncertain: false,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ProfilePlayRecordView::Ready {
        is_self,
        revision: None,
        items,
    })
}

impl MusicClient {
    pub async fn profile_play_record(
        &self,
        epoch: &str,
        uid: Option<&str>,
        period: ProfilePlayRecordPeriod,
    ) -> Result<AccountData<ProfilePlayRecordView>> {
        let token = self.playlist_token(epoch)?;
        let (uid, is_self) = target(&self.session, &token, uid)?;
        let store = self.session.persistent_store()?;
        let _lock = is_self
            .then(|| removal::lock_period(&store, &uid, period))
            .transpose()?;
        if is_self {
            self.session
                .with_token(&token, |_| self.profile_rank_removal.invalidate(period))?;
        }
        let mut value = self
            .session
            .run_for(&token, |api, _| {
                let token = &token;
                let uid = &uid;
                async move {
                    if !is_self && !visible(&self.session, token, &api, uid).await? {
                        return Ok(ProfilePlayRecordView::Denied);
                    }
                    read(api.as_ref(), uid, period, is_self).await
                }
            })
            .await?;
        self.session.with_token(&token, |uid| {
            if is_self {
                self.profile_rank_removal
                    .publish(&store, &token, uid, period, &mut value)?;
            }
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
