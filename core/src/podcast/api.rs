use super::{
    Episode, Radio,
    model::{count, rows},
};
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone)]
pub struct PodcastApi {
    pub(super) api: Arc<dyn Api>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgramPage {
    pub items: Vec<Episode>,
    pub total: u64,
    pub offset: usize,
    pub more: bool,
    pub ascending: bool,
}
impl PodcastApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn radio(&self, id: &Value) -> Result<Radio> {
        let id = identifier(id)?;
        let result = self
            .api
            .call("/api/djradio/v3/get", json!({"id":id}), false, &[200])
            .await?;
        let radio = Radio::from_official(&result["data"])?;
        if radio.id != id {
            return Err(Error::Response("podcast detail identity mismatch"));
        }
        Ok(radio)
    }
    pub async fn episode(&self, id: &Value) -> Result<Episode> {
        let id = identifier(id)?;
        let result = self
            .api
            .call("/api/dj/program/detail", json!({"id":id}), false, &[200])
            .await?;
        let item = Episode::from_official(&result["program"])?;
        if item.id != id {
            return Err(Error::Response("program detail identity mismatch"));
        }
        Ok(item)
    }
    /// M/15 Ur (export Ab), used by cross-device snapshot continuation. The
    /// official programIds value is a JSON-number-array STRING, not a string
    /// array or a conversion through machine-width/JavaScript numeric IDs.
    pub async fn episodes(&self, ids: &[String]) -> Result<Vec<Option<Episode>>> {
        if ids.is_empty() || ids.len() > 100 {
            return Err(Error::InvalidInput("invalid program batch size"));
        }
        for id in ids {
            identifier(&json!(id))?;
        }
        let result = self
            .api
            .call(
                "/api/dj/program/batch/detail",
                json!({"programIds":format!("[{}]",ids.join(","))}),
                false,
                &[200],
            )
            .await?;
        let rows = result["data"]
            .as_array()
            .filter(|r| r.len() <= ids.len())
            .ok_or(Error::Response("invalid program batch response"))?;
        let mut found = std::collections::BTreeMap::new();
        for row in rows {
            let episode = Episode::from_official(row)?;
            if !ids.contains(&episode.id) || found.insert(episode.id.clone(), episode).is_some() {
                return Err(Error::Response("program batch identity mismatch"));
            }
        }
        Ok(ids.iter().map(|id| found.get(id).cloned()).collect())
    }
    pub async fn programs(
        &self,
        id: &Value,
        limit: usize,
        offset: usize,
        ascending: bool,
    ) -> Result<ProgramPage> {
        if !(1..=200).contains(&limit) || offset > 1_000_000 {
            return Err(Error::InvalidInput("invalid program page"));
        }
        let id = identifier(id)?;
        let result = self
            .api
            .call(
                "/api/v6/dj/program/byradio",
                json!({"radioId":id,"limit":limit,"offset":offset,"asc":ascending}),
                false,
                &[200],
            )
            .await?;
        let data = &result["data"];
        let items = rows(data, "programs")?
            .iter()
            .map(Episode::from_official)
            .collect::<Result<Vec<_>>>()?;
        let mut seen = BTreeSet::new();
        // M15.Et maps every returned voice without replacing its original
        // radio. A curated radio can include episodes from another radio;
        // the containing list ID is not each episode's ownership identity.
        if items.len() > limit || items.iter().any(|v| !seen.insert(&v.id)) {
            return Err(Error::Response("program page identity mismatch"));
        }
        let more = data["more"]
            .as_bool()
            .ok_or(Error::Response("program page flag missing"))?;
        if more && items.is_empty() {
            return Err(Error::Response("program page does not advance"));
        }
        let asc = data["asc"]
            .as_bool()
            .ok_or(Error::Response("program order missing"))?;
        if asc != ascending {
            return Err(Error::Response("program order changed"));
        }
        Ok(ProgramPage {
            items,
            total: count(&data["count"])?,
            offset,
            more,
            ascending: asc,
        })
    }
    pub async fn resume_enabled(&self, id: &Value) -> Result<bool> {
        let result = self
            .api
            .call(
                "/api/dj/user/setting/resumeplay/get",
                json!({"radioId":identifier(id)?}),
                false,
                &[200],
            )
            .await?;
        match result["data"]["resumePlayValue"].as_u64() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(Error::Response("invalid podcast resume setting")),
        }
    }
    pub async fn skip_enabled(&self, id: &Value) -> Result<bool> {
        let id = identifier(id)?
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid numeric radio ID"))?;
        let result = self
            .api
            .call(
                "/api/voice/alg/skip/switch/get",
                json!({"radioId":id}),
                false,
                &[200],
            )
            .await?;
        result["data"]
            .as_bool()
            .ok_or(Error::Response("invalid podcast skip setting"))
    }
    pub async fn subscribe(&self, id: &Value, desired: bool) -> Result<()> {
        self.api
            .call(
                if desired {
                    "/api/djradio/sub"
                } else {
                    "/api/djradio/unsub"
                },
                json!({"id":identifier(id)?}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    pub async fn like(&self, id: &Value, desired: bool) -> Result<()> {
        self.api
            .call(
                if desired {
                    "/api/resource/like"
                } else {
                    "/api/resource/unlike"
                },
                json!({"threadId":format!("A_DJ_1_{}",identifier(id)?)}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    pub async fn set_resume_enabled(&self, id: &Value, desired: bool) -> Result<()> {
        self.api
            .call(
                "/api/dj/user/setting/resumeplay/update",
                json!({"radioId":identifier(id)?,"resumePlayValue":u8::from(desired)}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
}
