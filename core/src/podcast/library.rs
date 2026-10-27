use super::{
    Episode, PodcastApi, Radio,
    model::{count, rows},
};
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Podcast,
    Audiobook,
}
impl LibraryKind {
    fn select(self) -> u8 {
        match self {
            Self::Podcast => 1,
            Self::Audiobook => 2,
        }
    }
}
/// M/2170 + M/2090: the first ID snapshot remains authoritative when subsequent
/// responses omit/empty radioIdList. Preserve its original number/string types.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyLibrary {
    pub kind: LibraryKind,
    pub ids: Vec<Value>,
    pub items: Vec<Radio>,
    pub next_index: usize,
}
impl MyLibrary {
    pub fn more(&self) -> bool {
        self.next_index < self.ids.len()
    }
}
impl PodcastApi {
    pub async fn my_page(
        &self,
        kind: LibraryKind,
        previous: Option<&MyLibrary>,
    ) -> Result<MyLibrary> {
        let mut payload = json!({"selectType":kind.select(),"sortType":1});
        if let Some(previous) = previous {
            if previous.kind != kind || previous.next_index > previous.ids.len() {
                return Err(Error::InvalidInput("invalid podcast page snapshot"));
            }
            if !previous.more() {
                return Ok(previous.clone());
            }
            validate_ids(&previous.ids)?;
            payload["radioIdStr"] = json!(serde_json::to_string(
                &previous.ids
                    [previous.next_index..(previous.next_index + 20).min(previous.ids.len())]
            )?);
        }
        let result = self
            .api
            .call("/api/my/podcast/page/list", payload, false, &[200])
            .await?;
        let data = &result["data"];
        let page = rows(data, "myPodcastVOList")?
            .iter()
            .map(Radio::from_official)
            .collect::<Result<Vec<_>>>()?;
        let ids = match previous {
            Some(p) => p.ids.clone(),
            None => rows(data, "radioIdList")?.clone(),
        };
        let all_ids = validate_ids(&ids)?;
        if !ids.is_empty() && page.iter().any(|r| !all_ids.contains(&r.id)) {
            return Err(Error::Response("podcast card is outside ID snapshot"));
        }
        let mut items = previous.map(|p| p.items.clone()).unwrap_or_default();
        let mut seen = items.iter().map(|r| r.id.clone()).collect::<BTreeSet<_>>();
        for item in page {
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
        let next_index = previous
            .map(|p| p.next_index)
            .unwrap_or(0)
            .saturating_add(20)
            .min(ids.len());
        Ok(MyLibrary {
            kind,
            ids,
            items,
            next_index,
        })
    }
    pub async fn my_library(&self, kind: LibraryKind) -> Result<MyLibrary> {
        let mut result = self.my_page(kind, None).await?;
        while result.more() {
            result = self.my_page(kind, Some(&result)).await?;
        }
        Ok(result)
    }
    /// Old created/subscribed pages use time tuples, never offset.
    pub async fn legacy_library(&self, created: bool) -> Result<Vec<Radio>> {
        let limit = if created { 20 } else { 200 };
        let mut payload = json!({"limit":limit});
        let mut cursors = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut items = vec![];
        for _ in 0..2500 {
            let result = self
                .api
                .call(
                    if created {
                        "/api/social/my/created/voicelist/v1"
                    } else {
                        "/api/social/my/subscribed/voicelist/v1"
                    },
                    payload.clone(),
                    false,
                    &[200],
                )
                .await?;
            let data = &result["data"];
            let page = rows(data, "data")?;
            for raw in page {
                let item = Radio::from_official(raw)?;
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
            if !data["hasMore"]
                .as_bool()
                .ok_or(Error::Response("podcast library flag missing"))?
            {
                return Ok(items);
            }
            let last = page
                .last()
                .ok_or(Error::Response("podcast library cursor missing"))?;
            payload = json!({"limit":limit,"createTime":count(&last["createTime"])?,"lastProgramCreateTime":count(&last["lastProgramCreateTime"])?});
            if created {
                payload["lastPodcastId"] = json!(Radio::from_official(last)?.id);
            }
            if items.len() > 50_000 || !cursors.insert(serde_json::to_string(&payload)?) {
                return Err(Error::Response(
                    "podcast library pagination did not advance",
                ));
            }
        }
        Err(Error::Response("podcast library exceeds page limit"))
    }
    pub async fn purchased(&self) -> Result<Vec<Radio>> {
        let mut items = vec![];
        let mut seen = BTreeSet::new();
        for offset in (0..50_000).step_by(20) {
            let result = self
                .api
                .call(
                    "/api/voicelist/bought/list",
                    json!({"limit":20,"offset":offset}),
                    false,
                    &[200],
                )
                .await?;
            let data = &result["data"];
            let page = rows(data, "list")?;
            let before = items.len();
            for raw in page {
                let item = Radio::from_official(raw)?;
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
            if !data["hasMore"]
                .as_bool()
                .ok_or(Error::Response("purchased podcast page flag missing"))?
            {
                return Ok(items);
            }
            if items.len() == before {
                return Err(Error::Response(
                    "purchased podcast pagination did not advance",
                ));
            }
        }
        Err(Error::Response("purchased podcasts exceed page limit"))
    }
    pub async fn liked_voices(&self) -> Result<Vec<Episode>> {
        let mut cursor = json!(0);
        let mut cursors = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut items = vec![];
        for _ in 0..500 {
            let result = self
                .api
                .call(
                    "/api/content/my/liked/voice",
                    json!({"limit":100,"likeTime":cursor}),
                    false,
                    &[200],
                )
                .await?;
            let data = &result["data"];
            let page = rows(data, "data")?;
            for raw in page {
                let mut item = Episode::from_official(raw)?;
                item.liked = Some(true);
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
            if !data["hasMore"]
                .as_bool()
                .ok_or(Error::Response("liked voice page flag missing"))?
            {
                return Ok(items);
            }
            let next = page
                .last()
                .ok_or(Error::Response("liked voice cursor missing"))?["likeTime"]
                .clone();
            if count(&next)? == 0
                || next == cursor
                || !cursors.insert(serde_json::to_string(&next)?)
            {
                return Err(Error::Response("liked voice cursor did not advance"));
            }
            cursor = next;
        }
        Err(Error::Response("liked voices exceed page limit"))
    }
    pub async fn liked_count(&self) -> Result<u64> {
        let value = self
            .api
            .call(
                "/api/content/my/liked/voice/count",
                json!({}),
                false,
                &[200],
            )
            .await?;
        count(&value["data"])
    }
}
fn validate_ids(ids: &[Value]) -> Result<BTreeSet<String>> {
    if ids.len() > 50_000 {
        return Err(Error::Response("podcast ID snapshot exceeds limit"));
    }
    let mut set = BTreeSet::new();
    for id in ids {
        if !set.insert(identifier(id)?) {
            return Err(Error::Response("podcast ID snapshot repeats an identity"));
        }
    }
    Ok(set)
}
