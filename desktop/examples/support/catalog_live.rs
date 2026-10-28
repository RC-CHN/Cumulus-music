use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
pub struct Reads {
    api: MusicHttpClient,
    calls: Mutex<Vec<Value>>,
    output: std::path::PathBuf,
}
impl Reads {
    pub fn open(profile: &Path, output: &Path) -> Result<Self> {
        if !profile.join("core.sqlite3").is_file() {
            return Err(Error::InvalidInput("existing PC profile required"));
        }
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(profile)?),
            ApiOrigin::Pc,
        )?);
        Ok(Self {
            api: MusicHttpClient::new(credentials, ClientProfile::default())?,
            calls: Mutex::default(),
            output: output.to_owned(),
        })
    }
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/user/playlist"
                    | "/api/song/like/get"
                    | "/api/v3/discovery/recommend/songs"
                    | "/api/cloudsearch/pc"
                    | "/api/v3/song/detail"
                    | "/api/song/enhance/player/url/v1"
                    | "/api/song/lyric/v1"
                    | "/api/artist/v3/detail"
                    | "/api/artist/detail/dynamic"
                    | "/api/song/enhance/privilege"
                    | "/api/v2/artist/songs"
                    | "/api/artist/introduction"
                    | "/api/discovery/simiArtist"
                    | "/api/mlog/artist/video"
                    | "/api/concert/list/byartist"
                    | "/api/album/v3/detail"
                    | "/api/album/v4/detail"
                    | "/api/album/detail/dynamic"
                    | "/api/album/privilege"
                    | "/api/resource-exposure/config"
            ) && !path
                .strip_prefix("/api/artist/albums/")
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(Error::InvalidInput(
                    "catalog live proof only permits known read endpoints",
                ));
            }
            if path == "/api/resource-exposure/config"
                && (payload["resourcePosition"] != "album"
                    || payload["rexposureRecords"] != json!([]))
            {
                return Err(Error::InvalidInput(
                    "catalog exposure proof only reads album configuration",
                ));
            }
            let value = self.api.request(path, payload, cache).await?;
            let mut calls = self.calls.lock().map_err(|_| Error::LockPoisoned)?;
            calls.push(json!({"path":path,"code":value["code"]}));
            std::fs::write(
                self.output.join("api-calls.json"),
                serde_json::to_vec_pretty(&*calls)?,
            )?;
            Ok(value)
        })
    }
}
