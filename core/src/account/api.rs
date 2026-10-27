//! Account collections and remote settings from official M/232 and M/3097.
use crate::{
    Error, Result,
    protocol::{
        api::{Api, identifier},
        pagination,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Default)]
pub struct LikeContext {
    pub song_rq_refer: Option<Value>,
    pub rq_refer: Option<Value>,
    pub alg: Option<Value>,
}

#[derive(Clone)]
pub struct AccountApi {
    api: Arc<dyn Api>,
}
impl AccountApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }

    pub async fn liked_ids(&self, uid: &Value) -> Result<Vec<String>> {
        let response = self
            .api
            .call(
                "/api/song/like/get",
                json!({"uid":identifier(uid)?}),
                false,
                &[200],
            )
            .await?;
        response["ids"]
            .as_array()
            .ok_or(Error::Response("missing liked song IDs"))?
            .iter()
            .map(identifier)
            .collect()
    }

    pub async fn set_like(
        &self,
        uid: &Value,
        sid: &Value,
        liked: bool,
        context: LikeContext,
    ) -> Result<Value> {
        // M/232 O spells userid in lowercase. M/3160 accepts code 502, but
        // only a later resource read can establish the desired membership.
        let mut payload =
            json!({"trackId":identifier(sid)?,"userid":identifier(uid)?,"like":liked});
        for (key, value) in [
            ("songRqRefer", context.song_rq_refer),
            ("rqRefer", context.rq_refer),
            ("alg", context.alg),
        ] {
            if let Some(value) = value {
                payload[key] = value;
            }
        }
        self.api
            .call("/api/song/like", payload, false, &[200, 502])
            .await
    }

    pub async fn collected_albums(&self) -> Result<Vec<Value>> {
        pagination::all(
            self.api.as_ref(),
            "/api/album/sublist",
            json!({"total":true}),
            "data",
        )
        .await
    }

    pub async fn collected_artists(&self) -> Result<Vec<Value>> {
        pagination::all(self.api.as_ref(), "/api/artist/sublist", json!({}), "data").await
    }

    pub async fn collect_album(&self, aid: &Value, subscribed: bool) -> Result<Value> {
        let path = if subscribed {
            "/api/album/sub"
        } else {
            "/api/album/unsub"
        };
        self.api
            .call(path, json!({"id":identifier(aid)?}), false, &[200])
            .await
    }

    pub async fn collect_artist(&self, aid: &Value, subscribed: bool) -> Result<Value> {
        let aid = identifier(aid)?;
        if subscribed {
            self.api
                .call(
                    "/api/artist/sub",
                    json!({"artistId":aid}),
                    false,
                    &[200, 201],
                )
                .await
        } else {
            self.api
                .call(
                    "/api/artist/unsub",
                    json!({"artistIds":format!("[{aid}]")}),
                    false,
                    &[200],
                )
                .await
        }
    }

    pub async fn settings(&self) -> Result<Value> {
        let response = self
            .api
            .call("/api/user/setting", json!({}), false, &[200])
            .await?;
        response
            .get("setting")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(Error::Response("missing account settings"))
    }

    pub async fn update_settings(&self, patch: &Value) -> Result<Value> {
        validate_settings(patch)?;
        self.api
            .call("/api/user/setting/update", patch.clone(), false, &[200])
            .await
    }
}

pub fn validate_settings(patch: &Value) -> Result<()> {
    let map = patch
        .as_object()
        .filter(|map| !map.is_empty())
        .ok_or(Error::InvalidInput("expected a nonempty settings patch"))?;
    for (key, value) in map {
        let valid = match key.as_str() {
            "shareSetting" => value.as_i64().is_some_and(|n| [0, 1].contains(&n)),
            "playRecordSetting" => value.as_i64().is_some_and(|n| [0, 1, 2].contains(&n)),
            "personalServiceSetting" => value.as_i64().is_some_and(|n| [0, 2].contains(&n)),
            "allowSubscriptionNotify"
            | "allowLikedNotify"
            | "allowNewFollowerNotify"
            | "allowPeopleCanSeeMyPlayRecord" => value.is_boolean(),
            _ => false,
        };
        if !valid {
            return Err(Error::InvalidInput("unknown setting or invalid value"));
        }
    }
    Ok(())
}
