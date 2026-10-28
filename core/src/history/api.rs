use super::{HistoryKind, HistoryRecord, RecordOrigin, types::normalize};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    protocol::api::{Api, identifier, packed_ids},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryBoundary {
    pub id: String,
    pub played_at: u64,
}
pub struct HistoryApi {
    api: Arc<dyn Api>,
}
impl HistoryApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn recent(&self, kind: HistoryKind) -> Result<Vec<HistoryRecord>> {
        let (path, payload) = match kind {
            HistoryKind::Song => ("/api/play-record/song/list", json!({"limit":300})),
            HistoryKind::Album => ("/api/play-record/album/list", json!({"limit":100})),
            HistoryKind::Playlist => ("/api/play-record/playlist/list", json!({"limit":100})),
            HistoryKind::Voice => ("/api/play-record/voice/list", json!({})),
            HistoryKind::Podcast => (
                "/api/play-record/djradio/list",
                json!({"filterVoiceBook":true}),
            ),
            HistoryKind::Audiobook => ("/api/play-record/voice/book/timeline/list", json!({})),
        };
        // Official history has user-related, in-memory demotion caching, not
        // nginxCache. Recovery snapshots live in HistoryService, always refreshed.
        let value = self.api.call(path, payload, false, &[200]).await?;
        let rows: Vec<&Value> = if kind == HistoryKind::Audiobook {
            let data = &value["data"]["data"];
            let mut result = Vec::new();
            for key in [
                "todayRecords",
                "yesterdayRecords",
                "theDayBeforeYesterdayRecords",
                "earlierRecords",
            ] {
                result.extend(
                    data[key]
                        .as_array()
                        .ok_or(Error::Response("missing audiobook history group"))?,
                );
            }
            result
        } else {
            value["data"]["list"]
                .as_array()
                .ok_or(Error::Response("missing recent play records"))?
                .iter()
                .collect()
        };
        if rows.len() > 50_000 {
            return Err(Error::Response("recent play list exceeds limit"));
        }
        let mut records = Vec::new();
        for row in rows {
            if kind == HistoryKind::Podcast
                && row["resourceType"] != "DJ_RADIO"
                && row["data"]["resourceType"] != "DJ_RADIO"
            {
                continue;
            }
            let resource = if kind == HistoryKind::Voice {
                &row["data"]["pubDJProgramData"]
            } else {
                &row["data"]
            };
            // Official voice formatter returns no card when the program is absent.
            if kind == HistoryKind::Voice && resource.is_null() {
                continue;
            }
            let (id, resource) = normalize(kind, resource.clone(), false)?;
            let played_at = if kind == HistoryKind::Podcast && row["playTime"].is_null() {
                0
            } else {
                row["playTime"]
                    .as_u64()
                    .ok_or(Error::Response("invalid recent play time"))?
            };
            let device = if kind == HistoryKind::Audiobook {
                row["multiTerminalInfo"]["osText"].as_str()
            } else {
                row["multiTerminalInfo"]["os"].as_str().or_else(|| {
                    (kind == HistoryKind::Podcast)
                        .then(|| row["multiTerminalInfo"]["osText"].as_str())
                        .flatten()
                })
            };
            records.push(HistoryRecord {
                id,
                played_at,
                resource,
                device: device.map(str::to_owned),
                origin: RecordOrigin::Remote,
                play_progress: None,
                listened: None,
            });
        }
        // M/1384 fetchTrack -> M/86 f: merge privileges by ID without inventing
        // history entries from extra privilege rows or dropping original order.
        if kind == HistoryKind::Song && !records.is_empty() {
            let rights = CatalogApi::new(self.api.clone())
                .privileges(
                    &records.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                    None,
                )
                .await?;
            let by_id = rights
                .into_iter()
                .map(|v| identifier(&v["id"]).map(|id| (id, v)))
                .collect::<Result<BTreeMap<_, _>>>()?;
            for record in &mut records {
                record.resource["privilege"] =
                    by_id.get(&record.id).cloned().unwrap_or(Value::Null);
            }
        }
        Ok(records)
    }
    pub async fn clear(&self, kind: HistoryKind) -> Result<()> {
        self.api
            .call(
                "/api/play-record/clear",
                json!({"type":kind.wire()}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    /// Native low-level contract. Hosts must journal intent before calling;
    /// a lost response must never be retried as another deletion automatically.
    pub async fn delete(
        &self,
        kind: HistoryKind,
        ids: &[String],
        boundary: &HistoryBoundary,
    ) -> Result<()> {
        if ids.is_empty() || ids.len() > 50_000 {
            return Err(Error::InvalidInput("invalid history deletion selection"));
        }
        let values = ids.iter().map(|id| json!(id)).collect::<Vec<_>>();
        let packed = packed_ids(&values)?;
        let mut payload = json!({"type":kind.wire(),"lastPlayTime":boundary.played_at,"lastResourceId":identifier(&json!(boundary.id))?});
        let path = if kind == HistoryKind::Podcast && ids.len() == 1 {
            payload["resourceId"] = json!(identifier(&values[0])?);
            "/api/play-record/delete"
        } else {
            payload["resourceIds"] = json!(packed);
            "/api/play-record/batch/delete"
        };
        self.api.call(path, payload, false, &[200]).await?;
        Ok(())
    }
}
