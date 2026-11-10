//! M/15 and M/3008: a purchased page mixes radios and individual episodes.
use super::{
    Episode, PodcastApi, Radio,
    model::{count, rows},
};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(crate) const PAGE_SIZE: usize = 20;
const MAX_RAW_ROWS: usize = 50_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PurchasedKind {
    Radio,
    Episode,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "item")]
pub enum PurchasedItem {
    Radio(Radio),
    Episode(Episode),
}

impl PurchasedItem {
    pub fn identity(&self) -> (PurchasedKind, &str) {
        match self {
            Self::Radio(radio) => (PurchasedKind::Radio, &radio.id),
            Self::Episode(voice) => (PurchasedKind::Episode, &voice.id),
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Purchases {
    pub(crate) items: Vec<PurchasedItem>,
    pub(crate) total: Option<u64>,
    pub(crate) next_offset: Option<usize>,
    pages: Vec<String>,
}

impl PodcastApi {
    pub(super) async fn extend_purchases(
        &self,
        previous: Option<Purchases>,
        offset: usize,
    ) -> Result<Purchases> {
        if offset >= MAX_RAW_ROWS
            || previous.as_ref().map_or(Some(0), |old| old.next_offset) != Some(offset)
        {
            return Err(Error::QueueChanged);
        }
        let value = self
            .api
            .call(
                "/api/voicelist/bought/list",
                json!({"limit":PAGE_SIZE,"offset":offset}),
                false,
                &[200],
            )
            .await?;
        let data = &value["data"];
        let page = rows(data, "list")?;
        let more = data["hasMore"]
            .as_bool()
            .ok_or(Error::Response("purchased podcast page flag missing"))?;
        if page.len() > PAGE_SIZE || (more && page.is_empty()) {
            return Err(Error::Response("invalid purchased podcast page size"));
        }
        let mut result = previous.unwrap_or(Purchases {
            items: Vec::new(),
            total: None,
            next_offset: None,
            pages: Vec::new(),
        });
        let mut seen: BTreeSet<_> = result
            .items
            .iter()
            .map(|row| {
                let (kind, id) = row.identity();
                (kind, id.to_owned())
            })
            .collect();
        // Compare transport rows rather than supported rows: an explicit
        // newer product type can fill a page without authorizing any action.
        let signature = hex::encode(Sha256::digest(serde_json::to_vec(page)?));
        if !page.is_empty() && result.pages.contains(&signature) {
            return Err(Error::Response("purchased podcast page did not advance"));
        }
        result.pages.push(signature);
        for raw in page {
            let item = match raw.get("productType") {
                Some(kind) if kind.as_i64() == Some(1) => {
                    PurchasedItem::Episode(Episode::from_official(raw)?)
                }
                Some(kind) if kind.as_i64() == Some(0) => {
                    PurchasedItem::Radio(Radio::from_official(raw)?)
                }
                // Older radio-only payloads omit productType; M/15 uses its
                // default radio mapper. An explicit unknown type gets no action.
                None => PurchasedItem::Radio(Radio::from_official(raw)?),
                _ => continue,
            };
            let (kind, id) = item.identity();
            if id.len() > 32 || identifier(&json!(id)).is_err() {
                return Err(Error::Response("invalid purchased podcast identity"));
            }
            if seen.insert((kind, id.into())) {
                result.items.push(item);
            }
        }
        if !data["count"].is_null() {
            result.total = Some(count(&data["count"])?);
        }
        result.next_offset = more.then_some(offset + PAGE_SIZE);
        if result
            .next_offset
            .is_some_and(|offset| offset >= MAX_RAW_ROWS)
        {
            return Err(Error::Response("purchased podcasts exceed page limit"));
        }
        Ok(result)
    }
}
