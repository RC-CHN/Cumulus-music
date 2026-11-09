use crate::{Error, Result, catalog::CatalogApi, protocol::api::Api};
use chrono::Local;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) const GENERAL: &str = "/api/homepage/block/page";
pub(super) const CUSTOM: &str = "/api/pc/customize/block/page";
pub(super) const BOOKS: &str = "/api/podcast/common/single/block/get";
pub(super) const VOICES: &str = "/api/pc/voicelist/rcmd/list";
pub(super) const STYLE: &str = "HOMEPAGE_BLOCK_STYLE_RCMD";
pub(super) const VIP: &str = "HOMPAGE_BLOCK_VIP_RCMD";
pub(super) const PLAYLISTS: &str = "HOMEPAGE_BLOCK_PLAYLIST_RCMD";
pub(super) const HEARTBEAT: &str = "HOMEPAGE_BLOCK_RED_SIMILAR_SONG";
pub(super) const ALL_LISTEN: &str = "HOMEPAGE_BLOCK_ALL_LISTEN";
pub(super) const DAILY_VOICE: &str = "HOMEPAGE_MUSIC_PODCAST_RCMD_BLOCK";
pub(super) const HOT_RESOURCE: &str = "HOMEPAGE_BLOCK_HOT_RESOURCE";
const BLOCKS: &[&str] = &[VIP, HEARTBEAT, STYLE, DAILY_VOICE, PLAYLISTS, ALL_LISTEN];

pub(super) struct HomeFeed {
    pub batch: Value,
    pub mix: Option<Value>,
}

fn batch(hot: bool) -> Result<Value> {
    let mut blocks = BLOCKS.to_vec();
    if hot {
        blocks.push(HOT_RESOURCE);
    }
    let children = [
        ("/api/personalized/taste", json!({})),
        (CUSTOM, json!({"newStyle":true})),
        ("/api/playlist/tags", json!({})),
        (
            GENERAL,
            json!({
                "cursor": serde_json::to_string(&json!({"offset":0,"blockCodeOrderList":blocks}))?,
                "extInfo": r#"{"abInfo":{"hp-new-homepageV3.1":"t3"}}"#,
                "newStyle":true
            }),
        ),
        (
            BOOKS,
            json!({
                "pageCode":"PC_HOMEPAGE_PODCAST",
                "blockCode":"PC_HOMEPAGE_VOICEBOOK_RCMD",
                "extInfo":r#"{"position":"homePage"}"#
            }),
        ),
        (VOICES, json!({"limit":9})),
    ];
    let mut args = serde_json::Map::new();
    for (path, data) in children {
        // M/48 serializes each batch child's argument object into JSON text.
        args.insert(path.into(), serde_json::to_string(&data)?.into());
    }
    Ok(Value::Object(args))
}

pub(super) async fn feed(api: Arc<dyn Api>, hot: bool) -> Result<HomeFeed> {
    let batch = api.call("/api/batch", batch(hot)?, false, &[200]).await?;
    // M/244.T calls x after the batch. The official x treats failure as an
    // empty feature carousel rather than invalidating the other blocks.
    let local_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mix = optional(CatalogApi::new(api).daily_mix(&local_time).await)?;
    Ok(HomeFeed { batch, mix })
}

pub(super) async fn recent(api: Arc<dyn Api>) -> Result<Value> {
    api.call("/api/pc/recent/listen/list", json!({}), false, &[200])
        .await
}

pub(super) fn optional<T>(value: Result<T>) -> Result<Option<T>> {
    match value {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.is_service_code(301)
                || matches!(error, Error::StaleResponse | Error::CredentialChanged) =>
        {
            Err(error)
        }
        Err(_) => Ok(None),
    }
}
