use super::*;
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone)]
pub struct FmApi {
    api: Arc<dyn Api>,
}
impl FmApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn batch(&self, source: &FmSource, preload: bool) -> Result<Vec<FmTrack>> {
        source.validate()?;
        if let FmSource::Zone {
            rule_id,
            trial_mode,
        } = source
        {
            return self.zone_batch(rule_id, trial_mode.unwrap_or(-1)).await;
        }
        let mut args = json!({"imageFm":u8::from(preload)});
        if let FmSource::Familiar { source_id } = source {
            args["mode"] = json!("FAMILIAR");
            args["sourceIds"] = json!(source_id);
        }
        let value = self
            .api
            .call("/api/v1/radio/get", args, false, &[200])
            .await?;
        let data = value["data"]
            .as_array()
            .ok_or(Error::Response("missing FM recommendations"))?;
        data.iter()
            .map(|song| {
                Ok(FmTrack {
                    entry_id: uuid::Uuid::new_v4().simple().to_string(),
                    id: identifier(&song["id"])?,
                    algorithm: song["alg"].as_str().unwrap_or_default().into(),
                    song: song.clone(),
                })
            })
            .collect()
    }
    /// Official M1211.d -> M15.Pg -> M120.a. Zone recommendations are
    /// references, followed by details with their own trial context.
    async fn zone_batch(&self, rule_id: &str, trial_mode: i32) -> Result<Vec<FmTrack>> {
        let value = self.api.call("/api/tag/tab/fm/recommends",
            json!({"ruleType":1,"limit":3,"ruleId":rule_id,"firstSongId":"","firstSongAlg":"","tabTagId":""}),
            false, &[200]).await?;
        let references = value["data"]
            .as_array()
            .ok_or(Error::Response("missing zone FM references"))?;
        if references.len() > 500 {
            return Err(Error::Response("too many zone FM references"));
        }
        if references.is_empty() {
            return Ok(Vec::new());
        }
        let c = references
            .iter()
            .map(|v| Ok(json!({"id":identifier(&v["id"])?,"v":v["v"].as_u64().unwrap_or(0)})))
            .collect::<Result<Vec<_>>>()?;
        let details = self
            .api
            .call(
                "/api/v3/song/detail",
                json!({"c":serde_json::to_string(&c)?,"trialMode":trial_mode}),
                false,
                &[200],
            )
            .await?;
        let songs = details["songs"]
            .as_array()
            .ok_or(Error::Response("missing zone FM songs"))?;
        let mut rows = std::collections::BTreeMap::new();
        for song in songs {
            rows.insert(identifier(&song["id"])?, song);
        }
        let mut privileges = std::collections::BTreeMap::new();
        if let Some(list) = details.get("privileges") {
            for privilege in list
                .as_array()
                .ok_or(Error::Response("invalid zone FM privileges"))?
            {
                privileges.insert(identifier(&privilege["id"])?, privilege);
            }
        }
        let mut tracks = Vec::new();
        for item in c {
            let id = identifier(&item["id"])?;
            if let Some(song) = rows.get(&id) {
                let mut song = (*song).clone();
                if let Some(p) = privileges.get(&id) {
                    song["privilege"] = (*p).clone();
                }
                tracks.push(FmTrack {
                    entry_id: uuid::Uuid::new_v4().simple().to_string(),
                    id,
                    algorithm: song["alg"].as_str().unwrap_or_default().into(),
                    song,
                });
            }
        }
        Ok(tracks)
    }
    pub async fn skip(&self, report: &SkipReport) -> Result<()> {
        self.api.call("/api/v1/radio/skip",json!({"songId":identifier(&json!(report.song_id))?,"alg":report.algorithm,"time":report.seconds}),false,&[200]).await?;
        Ok(())
    }
    pub async fn trash(&self, id: &str, algorithm: &str, seconds: u64) -> Result<()> {
        self.api
            .call(
                "/api/radio/trash/add",
                json!({"songId":identifier(&json!(id))?,"alg":algorithm,"time":seconds}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    pub async fn restore(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() || ids.len() > 100 {
            return Err(Error::InvalidInput("invalid FM restore batch"));
        }
        let ids = ids
            .iter()
            .map(|id| identifier(&json!(id)))
            .collect::<Result<Vec<_>>>()?;
        self.api
            .call(
                "/api/radio/trash/del/batch",
                json!({"songIds":ids}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
    pub async fn trash_page(
        &self,
        page: usize,
        size: usize,
        add_time: Value,
    ) -> Result<FmTrashPage> {
        if page == 0
            || page > 10000
            || !(1..=100).contains(&size)
            || !(add_time.is_string() || add_time.is_number() || add_time.is_null())
        {
            return Err(Error::InvalidInput("invalid FM trash page"));
        }
        let v = self
            .api
            .call(
                "/api/v3/radio/trash/get",
                json!({"pagesize":size,"from":page-1,"to":page,"addTime":add_time}),
                false,
                &[200],
            )
            .await?;
        let total = v["count"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or(Error::Response("missing FM trash count"))?;
        let tracks = v["data"]
            .as_array()
            .ok_or(Error::Response("missing FM trash songs"))?
            .clone();
        for track in &tracks {
            identifier(&track["id"])?;
        }
        if tracks.len() > size {
            return Err(Error::Response("FM trash page exceeds page size"));
        }
        Ok(FmTrashPage {
            page,
            total,
            add_time: v["addTime"].clone(),
            tracks,
            more: total > page * size,
        })
    }
    pub async fn trash_snapshot(&self) -> Result<FmTrashSnapshot> {
        let mut ids = BTreeSet::new();
        let mut stamp = json!("");
        let mut total = None;
        let mut first_ids = Vec::new();
        for page in 1..=1000 {
            let result = self.trash_page(page, 100, stamp.clone()).await?;
            if total.is_some_and(|n| n != result.total) {
                return Err(Error::QueueChanged);
            }
            total = Some(result.total);
            stamp = result.add_time;
            let page_ids = result
                .tracks
                .iter()
                .map(|row| identifier(&row["id"]))
                .collect::<Result<Vec<_>>>()?;
            if page == 1 {
                first_ids = page_ids.clone();
            }
            for id in page_ids {
                if !ids.insert(id) {
                    return Err(Error::QueueChanged);
                }
            }
            // Live PC responses may contain songs with count=0. In that case
            // follow the official page cursor until an explicit empty page;
            // never treat the zero counter as proof of an empty blacklist.
            let exhausted = if result.total == 0 {
                result.tracks.is_empty()
            } else {
                !result.more
            };
            if exhausted {
                if result.total > 0 && ids.len() != result.total {
                    return Err(Error::QueueChanged);
                }
                if page > 1 {
                    let anchor = self.trash_page(1, 100, json!("")).await?;
                    let anchor_ids = anchor
                        .tracks
                        .iter()
                        .map(|row| identifier(&row["id"]))
                        .collect::<Result<Vec<_>>>()?;
                    if anchor.total != result.total || anchor_ids != first_ids {
                        return Err(Error::QueueChanged);
                    }
                }
                return Ok(FmTrashSnapshot {
                    total: ids.len(),
                    ids,
                    reported_total: result.total,
                    add_time: stamp,
                });
            }
            if result.total > 0 && result.tracks.len() != 100 {
                return Err(Error::Response("incomplete FM trash page"));
            }
        }
        Err(Error::State("FM trash scan limit reached"))
    }
    pub async fn blacklist(&self) -> Result<FmBlacklist> {
        let v = self
            .api
            .call("/api/music/dislike/get", json!({}), false, &[200])
            .await?;
        let ids = |name: &str| -> Result<BTreeSet<String>> {
            v["data"][name]
                .as_array()
                .ok_or(Error::Response("missing FM blacklist IDs"))?
                .iter()
                .map(|v| {
                    if name == "artistIdList" && (v.as_u64() == Some(0) || v.as_str() == Some("0"))
                    {
                        Ok("0".into())
                    } else {
                        identifier(v)
                    }
                })
                .collect()
        };
        Ok(FmBlacklist {
            songs: ids("songIdList")?,
            artists: ids("artistIdList")?,
        })
    }
    pub async fn reasons(&self, id: &str) -> Result<Value> {
        let id = identifier(&json!(id))?;
        self.api.call("/api/personalized/dislike/reason",json!({"resources":[serde_json::to_string(&json!({"resourceId":id,"resourceType":4,"targetUrl":"","encId":id}))?],"scene":"","page":"userfm"}),false,&[200]).await
    }
    pub async fn artist_blacklist(&self, id: &str, blocked: bool) -> Result<()> {
        let path = if blocked {
            "/api/music-blacklist/add"
        } else {
            "/api/music-blacklist/delete"
        };
        self.api
            .call(
                path,
                json!({"contentType":"artist", "contentIdList":[identifier(&json!(id))?]}),
                false,
                &[200],
            )
            .await?;
        Ok(())
    }
}
