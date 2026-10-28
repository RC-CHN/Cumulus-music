//! Synthetic history endpoints for real WebView UX checks. No network transport.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
};
#[derive(Default)]
pub struct Library {
    base: super::fixture::Library,
    mode: AtomicU8,
    removed: Mutex<BTreeMap<String, BTreeSet<String>>>,
    pub writes: AtomicUsize,
}
impl Library {
    pub fn mode(&self, value: &str) {
        self.mode.store(
            match value {
                "offline" => 1,
                "lost" => 2,
                "rejected" => 3,
                "empty" => 4,
                _ => 0,
            },
            Ordering::SeqCst,
        );
    }
}
fn resource(kind: &str, id: u64) -> Value {
    let name = if id == 1 {
        "臺灣 Ａ [Live] —— 很长的合成名称用于检查省略和按钮位置".into()
    } else {
        format!("合成记录 {id}")
    };
    match kind {
        "SONG" => {
            json!({"id":id,"name":name,"ar":[{"id":12,"name":"测试创作者"}],"al":{"id":7,"name":"测试专辑"},"dt":235000})
        }
        "ALBUM" => {
            json!({"albumId":id,"albumName":name,"artists":[{"id":12,"name":"测试创作者"}],"size":11})
        }
        "PLAYLIST" => {
            json!({"id":id,"name":name,"creator":{"nickname":"测试创作者"},"trackCount":11})
        }
        "VOICE" => {
            json!({"pubDJProgramData":{"voiceId":id,"voiceName":name,"mainSong":{"id":id+1000},"radio":{"id":8,"name":"测试播客"},"duration":235000}})
        }
        _ => {
            json!({"id":id,"name":name,"resourceType":kind,"programCount":11,"dj":{"nickname":"测试主播"}})
        }
    }
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mode = self.mode.load(Ordering::SeqCst);
            if path.starts_with("/api/play-record/") {
                if path.ends_with("/delete") || path.ends_with("/clear") {
                    self.writes.fetch_add(1, Ordering::SeqCst);
                    if mode == 3 {
                        return Ok(json!({"code":403}));
                    }
                    let kind = args["type"]
                        .as_str()
                        .ok_or(Error::Response("missing fixture kind"))?;
                    let ids: Vec<String> = if path.ends_with("/clear") {
                        (1..=300).map(|id| id.to_string()).collect()
                    } else if let Some(id) = args["resourceId"].as_str() {
                        vec![id.into()]
                    } else {
                        serde_json::from_str(args["resourceIds"].as_str().unwrap())?
                    };
                    self.removed
                        .lock()
                        .unwrap()
                        .entry(kind.into())
                        .or_default()
                        .extend(ids);
                    if mode == 2 {
                        return Err(Error::Transport("synthetic lost response".into()));
                    }
                    return Ok(json!({"code":200}));
                }
                if mode == 1 {
                    return Err(Error::Transport("synthetic offline".into()));
                }
                let kind = match path {
                    "/api/play-record/song/list" => "SONG",
                    "/api/play-record/album/list" => "ALBUM",
                    "/api/play-record/playlist/list" => "PLAYLIST",
                    "/api/play-record/voice/list" => "VOICE",
                    "/api/play-record/djradio/list" => "DJ_RADIO",
                    "/api/play-record/voice/book/timeline/list" => "VOICE_BOOK",
                    _ => return Err(Error::Response("unexpected history fixture endpoint")),
                };
                let removed = self.removed.lock().unwrap();
                let rows=(1..=if mode==4 {0}else if kind=="SONG"{300}else{100}).filter(|id|!removed.get(kind).is_some_and(|ids|ids.contains(&id.to_string())))
            .map(|id|json!({"data":resource(kind,id),"playTime":1793180000000_u64-id*1000,"multiTerminalInfo":{"os":if id%2==0{"android"}else{"windows"},"osText":"Windows"}})).collect::<Vec<_>>();
                return Ok(if kind == "VOICE_BOOK" {
                    json!({"code":200,"data":{"data":{"todayRecords":rows,"yesterdayRecords":[],"theDayBeforeYesterdayRecords":[],"earlierRecords":[]}}})
                } else {
                    json!({"code":200,"data":{"list":rows}})
                });
            }
            self.base.request(path, args, cache).await
        })
    }
}
