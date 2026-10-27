use super::{MvDetail, model::number};
use crate::{
    Error, Result,
    download::transfer::MediaUrl,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MvPurpose {
    Play,
    Download,
}
impl MvPurpose {
    pub fn path(self) -> &'static str {
        match self {
            Self::Play => "/api/song/enhance/play/mv/url",
            Self::Download => "/api/song/enhance/download/mv/url",
        }
    }
}
#[derive(Clone)]
pub struct MvApi {
    pub(super) api: Arc<dyn Api>,
}
impl MvApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn like(&self, id: &Value, desired: bool) -> Result<()> {
        let id = identifier(id)?;
        self.api
            .call(
                if desired {
                    "/api/resource/like"
                } else {
                    "/api/resource/unlike"
                },
                json!({"threadId":format!("R_MV_5_{id}")}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    /// M/3158 -> M/86.i -> M/15.Dg; default trial mode in M/91 is -1.
    pub async fn related_privileges(
        &self,
        ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, Value>> {
        let ids = ids
            .iter()
            .map(|id| identifier(&json!(id)))
            .collect::<Result<Vec<_>>>()?;
        if ids.len() > 5000 {
            return Err(Error::Response("too many MV related songs"));
        }
        let mut values = std::collections::BTreeMap::new();
        for batch in ids.chunks(500) {
            let result = self
                .api
                .call(
                    "/api/song/enhance/privilege",
                    json!({"ids":serde_json::to_string(batch)?,"trialMode":-1}),
                    false,
                    &[200],
                )
                .await?;
            for value in result["data"]
                .as_array()
                .ok_or(Error::Response("related song privileges missing"))?
            {
                let id = identifier(&value["id"])?;
                if !batch.contains(&id) || values.insert(id, value.clone()).is_some() {
                    return Err(Error::Response(
                        "unexpected related song privilege identity",
                    ));
                }
            }
        }
        Ok(values)
    }
    async fn detail_request(&self, id: &Value, options: bool) -> Result<MvDetail> {
        let id = identifier(id)?;
        let mut payload = json!({"id":id,"type":"2"});
        if options {
            payload["videoType"] = json!("mp4");
        }
        let result = self
            .api
            .call("/api/mlog/detail/v1", payload, false, &[200])
            .await?;
        let detail = MvDetail::from_official(&result["data"])?;
        if detail.id != id {
            return Err(Error::Response("MV detail belongs to another resource"));
        }
        Ok(detail)
    }
    pub async fn detail(&self, id: &Value) -> Result<MvDetail> {
        self.detail_request(id, false).await
    }
    pub async fn download_options(&self, id: &Value) -> Result<MvDetail> {
        self.detail_request(id, true).await
    }
    pub async fn collection(&self) -> Result<Vec<MvDetail>> {
        let mut cursor = json!("-1");
        let mut cursors = BTreeSet::new();
        let mut ids = BTreeSet::new();
        let mut items = vec![];
        for _ in 0..1000 {
            let result = self
                .api
                .call(
                    "/api/mlog/mycollect/bytime/get",
                    json!({"time":cursor,"mlogType":3,"limit":200}),
                    false,
                    &[200],
                )
                .await?;
            let data = &result["data"];
            let rows = data["feeds"]
                .as_array()
                .ok_or(Error::Response("MV collection feeds missing"))?;
            if rows.len() > 200 {
                return Err(Error::Response("MV collection page exceeds limit"));
            }
            for row in rows {
                let mut item = MvDetail::from_official(row)?;
                if !ids.insert(item.id.clone()) {
                    return Err(Error::Response("MV collection repeats a resource"));
                }
                // Collection membership is authoritative even if feed metadata
                // omits the detail-only subed field.
                item.subscribed = true;
                items.push(item);
            }
            let more = data["more"]
                .as_bool()
                .ok_or(Error::Response("MV collection pagination flag missing"))?;
            if !more {
                return Ok(items);
            }
            let next = &data["time"];
            let stamp = number(next)?
                .filter(|v| *v > 0)
                .ok_or(Error::Response("MV collection cursor missing"))?;
            if rows.is_empty()
                || !cursors.insert(stamp)
                || number(&cursor).ok().flatten() == Some(stamp)
            {
                return Err(Error::Response("MV collection cursor did not advance"));
            }
            cursor = next.clone();
        }
        Err(Error::Response("MV collection exceeds pagination limit"))
    }
    pub async fn collect(&self, id: &Value, desired: bool) -> Result<Value> {
        let id = identifier(id)?;
        let (path, payload) = if desired {
            ("/api/mv/sub", json!({"mvId":id}))
        } else {
            ("/api/mv/unsub", json!({"mvIds":[id]}))
        };
        self.api.call(path, payload, false, &[200]).await
    }
    /// Native authorization record; do not serialize its URL across IPC.
    pub async fn authorize(&self, id: &Value, br: u32, purpose: MvPurpose) -> Result<Value> {
        if br == 0 || br > 8640 {
            return Err(Error::InvalidInput("invalid MV resolution"));
        }
        let id = identifier(id)?;
        let result = self
            .api
            .call(purpose.path(), json!({"id":id,"r":br}), false, &[200])
            .await?;
        let data = result
            .get("data")
            .filter(|v| v.is_object())
            .ok_or(Error::Response("MV authorization missing"))?;
        if data["code"] != 200 {
            return Err(Error::service(
                purpose.path(),
                data["code"].clone(),
                data.clone(),
            ));
        }
        if identifier(&data["id"])? != id {
            return Err(Error::Response("MV authorization identity mismatch"));
        }
        if !data["freeTrialInfo"].is_null() {
            return Err(Error::Response("MV trial resource is not full media"));
        }
        MediaUrl::parse(
            data["url"]
                .as_str()
                .ok_or(Error::Response("MV media URL missing"))?,
        )?;
        number(&data["r"])?
            .filter(|v| *v > 0 && *v <= 8640)
            .ok_or(Error::Response("MV actual resolution missing"))?;
        if purpose == MvPurpose::Download {
            number(&data["size"])?
                .filter(|v| *v > 0)
                .ok_or(Error::Response("MV download size missing"))?;
        }
        if let Some(md5) = data.get("md5").filter(|v| !v.is_null()) {
            let value = md5.as_str().ok_or(Error::Response("invalid MV digest"))?;
            if !value.is_empty()
                && (value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err(Error::Response("invalid MV digest"));
            }
        }
        let mut normalized = data.clone();
        normalized["r"] = json!(number(&data["r"])?.expect("validated"));
        if let Some(size) = number(&data["size"])? {
            normalized["size"] = json!(size);
        }
        Ok(normalized)
    }
}
