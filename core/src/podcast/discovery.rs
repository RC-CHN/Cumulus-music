use super::{PodcastApi, Radio, model::rows};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveryPage {
    pub records: Vec<Value>,
    pub cursor: String,
    pub more: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum HistoryKind {
    Album,
    Podcast,
    Voice,
    Audiobook,
}
impl PodcastApi {
    pub async fn home(&self, cursor: &str) -> Result<DiscoveryPage> {
        cursor_valid(cursor)?;
        let value = self
            .api
            .call(
                "/api/voice/homepage/block/page",
                json!({"cursor":cursor}),
                false,
                &[200],
            )
            .await?;
        let data = &value["data"];
        // Repeated block IDs can contain different children. Retain every block.
        let records = rows(data, "blocks")?.clone();
        page(records, &data["cursor"], &data["hasMore"], cursor)
    }
    pub async fn categories(&self) -> Result<Vec<Value>> {
        let value = self
            .api
            .call("/api/dj/radio/category/list", json!({}), false, &[200])
            .await?;
        Ok(rows(&value["data"], "parentCategoryVoList")?.clone())
    }
    pub async fn category(
        &self,
        parent_id: &str,
        sub_id: &str,
        cursor: &str,
        limit: usize,
    ) -> Result<DiscoveryPage> {
        cursor_valid(cursor)?;
        // The official all-subcategories sentinel is the text "000".
        // Numeric conversion would destroy this contract.
        for id in [parent_id, sub_id] {
            if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::InvalidInput("invalid podcast category identifier"));
            }
        }
        if !(1..=200).contains(&limit) {
            return Err(Error::InvalidInput("invalid podcast category limit"));
        }
        let value = self
            .api
            .call(
                "/api/dj/radio/category/radio/list",
                json!({"parentId":parent_id,"subId":sub_id,"cursor":cursor,"limit":limit}),
                false,
                &[200],
            )
            .await?;
        let data = &value["data"];
        // The service can return far more than limit (observed 175 for limit=5).
        // M/597 concatenates pages; preserve overlaps and server cursor semantics.
        let records = rows(data, "radioList")?
            .iter()
            .map(|v| Radio::from_official(v).map(|r| r.normalized()))
            .collect::<Result<Vec<_>>>()?;
        page(
            records,
            &data["page"]["cursor"],
            &data["page"]["more"],
            cursor,
        )
    }
    pub async fn history(&self, kind: HistoryKind) -> Result<Value> {
        let (path, payload) = match kind {
            HistoryKind::Album => ("/api/play-record/album/list", json!({"limit":100})),
            HistoryKind::Podcast => (
                "/api/play-record/djradio/list",
                json!({"filterVoiceBook":true}),
            ),
            HistoryKind::Voice => ("/api/play-record/voice/list", json!({})),
            HistoryKind::Audiobook => ("/api/play-record/voice/book/timeline/list", json!({})),
        };
        let value = self.api.call(path, payload, false, &[200]).await?;
        match kind {
            HistoryKind::Audiobook => value["data"]
                .get("data")
                .cloned()
                .ok_or(Error::Response("audiobook timeline missing")),
            HistoryKind::Podcast => Ok(json!(
                rows(&value["data"], "list")?
                    .iter()
                    .filter(|v| v["resourceType"] == "DJ_RADIO"
                        || v["data"]["resourceType"] == "DJ_RADIO")
                    .collect::<Vec<_>>()
            )),
            _ => Ok(json!(rows(&value["data"], "list")?)),
        }
    }
}
fn cursor_valid(cursor: &str) -> Result<()> {
    if cursor.len() > 16_384 {
        return Err(Error::InvalidInput("podcast cursor exceeds limit"));
    }
    Ok(())
}
fn page(
    records: Vec<Value>,
    cursor: &Value,
    more: &Value,
    previous: &str,
) -> Result<DiscoveryPage> {
    let more = more
        .as_bool()
        .ok_or(Error::Response("podcast discovery page flag missing"))?;
    let cursor = match cursor {
        Value::String(s) => s.clone(),
        Value::Null if !more => String::new(),
        _ => return Err(Error::Response("podcast discovery cursor missing")),
    };
    cursor_valid(&cursor)?;
    if records.len() > 10_000
        || more && (cursor.is_empty() || cursor == previous || records.is_empty())
    {
        return Err(Error::Response("podcast discovery did not advance"));
    }
    Ok(DiscoveryPage {
        records,
        cursor,
        more,
    })
}
