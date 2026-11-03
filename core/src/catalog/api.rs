use crate::{
    Error, Result,
    protocol::api::{Api, identifier, packed_ids},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Copy)]
pub struct SearchOptions {
    pub kind: u32,
    pub limit: usize,
    pub offset: usize,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            kind: 1,
            limit: 30,
            offset: 0,
        }
    }
}

pub struct PlayOptions<'a> {
    pub level: &'a str,
    pub encode_type: &'a str,
    pub trial_mode: i32,
}
impl Default for PlayOptions<'_> {
    fn default() -> Self {
        Self {
            level: "standard",
            encode_type: "aac",
            trial_mode: -1,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SongBatch {
    pub songs: Vec<Option<Value>>,
    pub privileges: BTreeMap<String, Value>,
    pub missing: Vec<String>,
}

#[derive(Clone)]
pub struct CatalogApi {
    pub(super) api: Arc<dyn Api>,
}
/// M/244 chooses v3 only for PH-PC-Blacklist=t1; v1 is the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DailyRecommendationEndpoint {
    V1,
    V3,
}

impl DailyRecommendationEndpoint {
    fn path(self) -> &'static str {
        match self {
            Self::V1 => "/api/v1/discovery/recommend/songs",
            Self::V3 => "/api/v3/discovery/recommend/songs",
        }
    }
}

pub struct DailyRecommendationReply {
    pub songs: Vec<Value>,
    pub show_free_tag: bool,
}

impl CatalogApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }

    pub async fn songs(&self, ids: &[Value], batch_size: usize) -> Result<SongBatch> {
        if !(1..=500).contains(&batch_size) {
            return Err(Error::InvalidInput("invalid song batch size"));
        }
        let ids = ids.iter().map(identifier).collect::<Result<Vec<_>>>()?;
        let mut seen = HashSet::new();
        let unique: Vec<_> = ids.iter().filter(|id| seen.insert((*id).clone())).collect();
        let mut songs = BTreeMap::new();
        let mut privileges = BTreeMap::new();
        for chunk in unique.chunks(batch_size) {
            let c: Vec<_> = chunk.iter().map(|id| json!({"id":id,"v":0})).collect();
            let response = self
                .api
                .call(
                    "/api/v3/song/detail",
                    json!({"c":serde_json::to_string(&c)?}),
                    false,
                    &[200],
                )
                .await?;
            let rows = response["songs"]
                .as_array()
                .ok_or(Error::Response("missing song list"))?;
            for song in rows {
                songs.insert(identifier(&song["id"])?, song.clone());
            }
            if let Some(values) = response.get("privileges") {
                for privilege in values
                    .as_array()
                    .ok_or(Error::Response("invalid privilege list"))?
                {
                    privileges.insert(identifier(&privilege["id"])?, privilege.clone());
                }
            }
        }
        Ok(SongBatch {
            missing: unique
                .into_iter()
                .filter(|id| !songs.contains_key(*id))
                .cloned()
                .collect(),
            songs: ids.iter().map(|id| songs.get(id).cloned()).collect(),
            privileges,
        })
    }

    pub async fn search(&self, keyword: &str, options: SearchOptions) -> Result<Value> {
        if keyword.trim().is_empty() || !(1..=100).contains(&options.limit) {
            return Err(Error::InvalidInput("invalid search input"));
        }
        self.api.call("/api/cloudsearch/pc", json!({"s":keyword,"type":options.kind,"limit":options.limit,"offset":options.offset,"total":true}),false,&[200]).await
    }

    /// Historical raw v3 API used by existing independent callers and fixtures.
    pub async fn daily(&self) -> Result<Value> {
        self.daily_response(DailyRecommendationEndpoint::V3).await
    }

    async fn daily_response(&self, endpoint: DailyRecommendationEndpoint) -> Result<Value> {
        self.api
            .call(endpoint.path(), json!({"limit":30}), false, &[200])
            .await
    }

    /// M/15 Ph/ii have distinct list locations. No alternate endpoint retry.
    pub async fn daily_for(
        &self,
        endpoint: DailyRecommendationEndpoint,
    ) -> Result<DailyRecommendationReply> {
        let mut response = self.daily_response(endpoint).await?;
        let show_free_tag = response["data"]["showFreeTag"].as_bool().unwrap_or(false);
        let value = match endpoint {
            DailyRecommendationEndpoint::V1 => response.get_mut("recommend"),
            DailyRecommendationEndpoint::V3 => response
                .get_mut("data")
                .and_then(|data| data.get_mut("dailySongs")),
        };
        let Some(Value::Array(songs)) = value.map(Value::take) else {
            return Err(Error::Response("missing daily recommendation songs"));
        };
        if songs.len() > 500 {
            return Err(Error::Response("too many daily recommendation songs"));
        }
        Ok(DailyRecommendationReply {
            songs,
            show_free_tag,
        })
    }

    pub(crate) async fn daily_mix(&self, client_time: &str) -> Result<Value> {
        self.api
            .call(
                "/api/pc/daily/rcmd/block",
                json!({"clientTime":client_time}),
                false,
                &[200],
            )
            .await
    }

    pub(crate) async fn seeded_style_recommendations(&self, data: Value) -> Result<Value> {
        self.api
            .call(
                "/api/homepage/category/daily/song/list",
                data,
                false,
                &[200],
            )
            .await
    }

    /// M/3126 daily-entry defaults. M/12 folds the query into the API payload.
    pub async fn default_style_recommendations(&self) -> Result<Value> {
        self.api
            .call(
                "/api/homepage/category/daily/song/list",
                json!({"source":"","categoryId":"","tagId":"","songId":""}),
                false,
                &[200],
            )
            .await
    }

    /// M/3126 -> M/3014 dispatches the post-save recommendation read with data: {}.
    pub(crate) async fn style_recommendations_after_save(&self) -> Result<Value> {
        self.api
            .call(
                "/api/homepage/category/daily/song/list",
                json!({}),
                false,
                &[200],
            )
            .await
    }

    pub async fn album(&self, id: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/album/v3/detail",
                json!({"id":identifier(id)?}),
                true,
                &[200],
            )
            .await
    }

    pub async fn artist(&self, id: &Value) -> Result<Value> {
        // M/612: artist/hotSongs are top-level and nginxCache is enabled.
        let result = self
            .api
            .call(
                "/api/artist/v3/detail",
                json!({"id":identifier(id)?,"top":50,"e_r":false}),
                true,
                &[200],
            )
            .await?;
        if !result["artist"].is_object() || !result["hotSongs"].is_array() {
            return Err(Error::Response("missing artist or hot songs"));
        }
        Ok(result)
    }

    pub async fn lyric(&self, id: &Value) -> Result<Value> {
        self.api.call("/api/song/lyric/v1",json!({"id":identifier(id)?,"cp":false,"tv":-1,"lv":-1,"rv":-1,"kv":-1,"yv":-1,"ytyv":-1,"ytlv":-1,"ytrv":-1}),false,&[200]).await
    }

    pub async fn play_url(&self, id: &Value, options: PlayOptions<'_>) -> Result<Option<Value>> {
        // Official M/91 scene enum. Context comes from the actual source.
        if ![-1, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 121].contains(&options.trial_mode) {
            return Err(Error::InvalidInput("unknown playback trial scene"));
        }
        let id_text = identifier(id)?;
        let result = self.api.call("/api/song/enhance/player/url/v1",json!({"ids":packed_ids(std::slice::from_ref(id))?,"level":options.level,"encodeType":options.encode_type,"immerseType":"c51","trialMode":options.trial_mode}),false,&[200]).await?;
        let data = result["data"]
            .as_array()
            .ok_or(Error::Response("missing playback resources"))?;
        Ok(data
            .iter()
            .find(|row| identifier(&row["id"]).is_ok_and(|value| value == id_text))
            .cloned())
    }

    pub async fn download_url(&self, id: &Value, level: &str) -> Result<Option<Value>> {
        let result = self
            .api
            .call(
                "/api/song/enhance/download/url/v1",
                json!({"id":identifier(id)?,"level":level,"immerseType":"c51"}),
                false,
                &[200],
            )
            .await?;
        Ok(result.get("data").filter(|value| !value.is_null()).cloned())
    }
}
