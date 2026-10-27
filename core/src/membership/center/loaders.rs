use super::banners;
use crate::{
    Error, Result,
    catalog::CatalogApi,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const ROOT: &str = "/api/vipnewcenter/app/";
async fn data(api: &dyn Api, suffix: &str, params: Value) -> Result<Value> {
    Ok(api
        .call(&format!("{ROOT}{suffix}"), params, false, &[200])
        .await?
        .get("data")
        .cloned()
        .unwrap_or(Value::Null))
}
fn array_or_empty(value: Value) -> Result<Value> {
    match value {
        Value::Null => Ok(json!([])),
        Value::Array(_) => Ok(value),
        _ => Err(Error::Response("invalid member content array")),
    }
}
async fn ranking(api: Arc<dyn Api>) -> Result<Value> {
    let mut rows = array_or_empty(data(api.as_ref(), "viptab/ranking/list", json!({})).await?)?;
    let groups = rows
        .as_array_mut()
        .ok_or(Error::Response("invalid ranking array"))?;
    let mut ids = vec![];
    for group in groups.iter() {
        for item in group["contentList"]
            .as_array()
            .ok_or(Error::Response("invalid ranking contents"))?
        {
            ids.push(item["songId"].clone());
        }
    }
    let songs = CatalogApi::new(api).songs(&ids, 500).await?;
    let mut by_id = BTreeMap::new();
    for song in songs.songs.into_iter().flatten() {
        by_id.insert(identifier(&song["id"])?, song);
    }
    for group in groups.iter_mut() {
        for item in group["contentList"]
            .as_array_mut()
            .ok_or(Error::Response("invalid ranking contents"))?
        {
            let id = identifier(&item["songId"])?;
            item["track"] = by_id.get(&id).cloned().unwrap_or(Value::Null);
        }
    }
    Ok(json!({"kind":"rankList","value":rows}))
}
async fn module(api: Arc<dyn Api>, kind: u64) -> Result<Value> {
    match kind {
        1 | 7 => banners::normalize(
            &data(
                api.as_ref(),
                "resource/common/list",
                json!({"positionType":if kind==1 {16} else {18}}),
            )
            .await?,
            kind == 1,
        ),
        3 | 4 => {
            let result = data(
                api.as_ref(),
                "viptab/recommend/song/list",
                json!({"from":1}),
            )
            .await?;
            Ok(json!({"kind":"rcmdTracks","value":array_or_empty(result["songInfoList"].clone())?}))
        }
        5 => Ok(
            json!({"kind":"audioBooks","value":api.call("/api/podcast/common/single/block/get",json!({"blockCode":"VIP_THEATER_VOICE_BOOK","pageCode":"HOMEPAGE_VOICELIST_RCMD"}),false,&[200]).await?}),
        ),
        6 => Ok(
            json!({"kind":"digitAlbums","value":array_or_empty(data(api.as_ref(),"resource/plus/recomend/album",json!({})).await?)?}),
        ),
        9 => ranking(api).await,
        12 => Ok(
            json!({"kind":"playlist","value":array_or_empty(data(api.as_ref(),"viptab/recommend/playList/list",json!({})).await?)?}),
        ),
        _ => Err(Error::State("unsupported member module loader")),
    }
}
pub(super) async fn load(api: Arc<dyn Api>) -> Result<Value> {
    let modules = data(api.as_ref(), "viptab/module/get", json!({})).await?;
    let modules = modules
        .as_array()
        .ok_or(Error::Response("member module list is missing"))?;
    let carousel = data(api.as_ref(), "resource/centertop/carousel/list", json!({})).await?;
    let mut values = BTreeMap::new();
    let mut results = vec![];
    for item in modules {
        let Some(kind) = item["moduleType"]
            .as_u64()
            .filter(|n| [1, 3, 4, 5, 6, 7, 9, 12].contains(n))
        else {
            continue;
        };
        let key = if kind == 4 { 3 } else { kind };
        if let std::collections::btree_map::Entry::Vacant(entry) = values.entry(key) {
            entry.insert(module(api.clone(), kind).await?);
        }
        results.push(json!({"module":item,"value":values[&key]}));
    }
    Ok(
        json!({"status":"ready","results":results,"carouselList":carousel,"fetched":SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|Error::State("system clock precedes UNIX epoch"))?.as_secs_f64(),"moduleTypes":modules.iter().map(|m|m["moduleType"].clone()).collect::<Vec<_>>()}),
    )
}
