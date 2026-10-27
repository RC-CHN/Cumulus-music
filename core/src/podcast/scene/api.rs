use super::*;
use crate::{Error, Result, podcast::Episode, protocol::api::Api};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};
pub struct SceneFmApi {
    api: Arc<dyn Api>,
}
fn voices(rows: &Value) -> Result<Vec<Episode>> {
    let rows = rows
        .as_array()
        .ok_or(Error::Response("missing scene FM programs"))?;
    if rows.len() > 5000 {
        return Err(Error::Response("scene FM queue exceeds limit"));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for row in rows {
        let v = Episode::from_official(row)?;
        if !seen.insert(v.id.clone()) {
            return Err(Error::Response("duplicate scene FM program"));
        }
        out.push(v);
    }
    Ok(out)
}
impl SceneFmApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn home(&self) -> Result<Vec<SceneFmCategory>> {
        let v = self
            .api
            .call(
                "/api/voice/homepage/block/content",
                json!({"blockCode":"VOICE_HOMEPAGE_BLOCK_SCENE_FM"}),
                false,
                &[200],
            )
            .await?;
        let block = &v["data"][0];
        let rows = block["creatives"]
            .as_array()
            .ok_or(Error::Response("missing scene FM creatives"))?;
        if rows.len() > 200 {
            return Err(Error::Response("too many scene FM categories"));
        }
        let mut scenes = Vec::new();
        let mut seen = BTreeSet::new();
        for r in rows {
            let Some(id) = r["creativeId"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            scene_id(id)?;
            if !seen.insert(id) {
                return Err(Error::Response("duplicate scene FM category"));
            }
            let list = &r["creativeExtInfoVO"]["djPrograms"];
            scenes.push(SceneFmCategory {
                id: id.into(),
                name: r["uiElement"]["mainTitle"]["title"]
                    .as_str()
                    .unwrap_or(id)
                    .into(),
                label: r["uiElement"]["labelTexts"][0]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                algorithm: r["alg"].as_str().unwrap_or_default().into(),
                voices: if list.is_null() {
                    vec![]
                } else {
                    voices(list)?
                },
                selected: None,
            });
        }
        Ok(scenes)
    }
    pub async fn programs(&self, id: &str) -> Result<Vec<Episode>> {
        scene_id(id)?;
        let v = self
            .api
            .call(
                "/api/content/scene/fm/play/list",
                json!({"sceneCode":"SCENE_FM","sceneId":id}),
                false,
                &[200],
            )
            .await?;
        voices(&v["data"]["list"])
    }
}
