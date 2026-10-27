//! Supplemental endpoint: archived YesPlayMusic simiMv and API-Reborn simi_mv.
//! No equivalent request was present in the recovered official desktop modules.
use super::{MvApi, MvService, model::number};
use crate::{Error, Result, account::SessionToken, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Clone, Serialize, Deserialize)]
pub struct MvRecommendation {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub cover: Option<String>,
    pub duration_ms: u64,
    pub play_count: u64,
}
impl MvApi {
    pub async fn recommendations(&self, id: &Value) -> Result<Vec<MvRecommendation>> {
        let id = identifier(id)?;
        let value = self
            .api
            .call("/api/discovery/simiMV", json!({"mvid":id}), false, &[200])
            .await?;
        let rows = value["mvs"]
            .as_array()
            .ok_or(Error::Response("similar MV list missing"))?;
        if rows.len() > 100 {
            return Err(Error::Response("similar MV list too large"));
        }
        let mut seen = std::collections::BTreeSet::new();
        rows.iter()
            .map(|v| {
                let id = identifier(&v["id"])?;
                if !seen.insert(id.clone()) {
                    return Err(Error::Response("similar MV list repeats identifier"));
                }
                let artists = match v.get("artists").filter(|v| !v.is_null()) {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|a| a["name"].as_str().unwrap_or_default().to_owned())
                        .collect(),
                    None => vec![v["artistName"].as_str().unwrap_or_default().into()],
                    _ => return Err(Error::Response("invalid similar MV artists")),
                };
                Ok(MvRecommendation {
                    id,
                    title: v["name"].as_str().unwrap_or_default().into(),
                    artists,
                    cover: v["cover"].as_str().map(str::to_owned),
                    duration_ms: number(&v["duration"])?.unwrap_or(0),
                    play_count: number(&v["playCount"])?.unwrap_or(0),
                })
            })
            .collect()
    }
}
const CACHE: &str = "mv-recommendations-v1";
impl MvService {
    pub async fn recommendations_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Vec<MvRecommendation>> {
        let id = identifier(&json!(id))?;
        let rows = self
            .session
            .run_for(token, |api, _| async {
                MvApi::new(api).recommendations(&json!(id)).await
            })
            .await?;
        self.session
            .save_result(token, CACHE, &id, &json!({"id":id,"rows":rows}))?;
        Ok(rows)
    }
    pub fn cached_recommendations_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<Option<Vec<MvRecommendation>>> {
        let id = identifier(&json!(id))?;
        self.session.with_token(token, |owner| {
            self.store
                .get(owner, CACHE, &id)?
                .map(|v| {
                    if v["id"] != id {
                        return Err(Error::State("MV recommendations cache identity mismatch"));
                    }
                    Ok(serde_json::from_value(v["rows"].clone())?)
                })
                .transpose()
        })
    }
}
