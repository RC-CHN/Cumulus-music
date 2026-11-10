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
const LIKED_PAGE_SIZE: usize = 100;

pub(crate) struct LikedVoicePage {
    pub rows: Vec<Episode>,
    pub next: Option<(Value, u64)>,
    pub count: Option<u64>,
}

impl PodcastApi {
    async fn liked_voice_response(&self, cursor: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/content/my/liked/voice",
                json!({"limit":LIKED_PAGE_SIZE,"likeTime":cursor}),
                false,
                &[200],
            )
            .await
    }

    // M3149: one uncached display page. The cursor comes from the raw tail,
    // even when the display prefix already contains that programme.
    pub(crate) async fn liked_voice_page(&self, cursor: &Value) -> Result<LikedVoicePage> {
        let result = self.liked_voice_response(cursor).await?;
        let data = &result["data"];
        let page = rows(data, "data")?;
        if page.len() > LIKED_PAGE_SIZE {
            return Err(Error::Response("liked voice display page exceeds limit"));
        }
        let more = data["hasMore"]
            .as_bool()
            .ok_or(Error::Response("liked voice page flag missing"))?;
        let next = if more {
            let raw = page
                .last()
                .ok_or(Error::Response("liked voice cursor missing"))?["likeTime"]
                .clone();
            let time = count(&raw)?;
            if time == 0 {
                return Err(Error::Response("liked voice cursor missing"));
            }
            Some((raw, time))
        } else {
            None
        };
        let rows = page
            .iter()
            .map(|raw| {
                let mut item = Episode::from_official(raw)?;
                item.liked = Some(true);
                Ok(item)
            })
            .collect::<Result<_>>()?;
        Ok(LikedVoicePage {
            rows,
            next,
            count: (!data["count"].is_null())
                .then(|| count(&data["count"]))
                .transpose()?,
        })
    }

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
    pub async fn liked_voices(&self) -> Result<Vec<Episode>> {
        let mut cursor = json!(0);
        let mut cursors = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut items = vec![];
        for _ in 0..500 {
            let result = self.liked_voice_response(&cursor).await?;
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
        if value["data"].is_null() {
            return Err(Error::Response("liked voice count missing"));
        }
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
