//! Synthetic history endpoints for real WebView UX checks. No network transport.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture, GuardedPlaylistAdd, PlaylistAddPermission},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
};
#[derive(Default)]
pub struct Library {
    base: super::fixture::Library,
    mode: AtomicU8,
    chapters: bool,
    collections: bool,
    menu: bool,
    pub row_queue_menu: bool,
    pub row_menu: super::history_row_menu_fixture::Fixture,
    chapter_calls: Mutex<Vec<Value>>,
    collected: Mutex<BTreeSet<String>>,
    removed: Mutex<BTreeMap<String, BTreeSet<String>>>,
    pub reads: AtomicUsize,
    pub writes: AtomicUsize,
    pub downloads: Option<Arc<super::history_download_fixture::Fixture>>,
    pub song_collect: Option<super::history_song_collect_fixture::Fixture>,
    pub song_download: Option<Arc<super::history_song_download_fixture::Fixture>>,
}
impl Library {
    pub fn with_chapters() -> Self {
        Self {
            chapters: true,
            ..Self::default()
        }
    }
    pub fn with_collections() -> Self {
        Self {
            collections: true,
            ..Self::with_chapters()
        }
    }

    pub fn with_radio_menu() -> Self {
        Self {
            menu: true,
            ..Self::with_collections()
        }
    }

    pub fn chapter_report(&self) -> Value {
        json!(self.chapter_calls.lock().unwrap().clone())
    }

    fn chapter_response(
        &self,
        path: &str,
        args: &Value,
        cached: bool,
    ) -> Option<cumulus_core::Result<Value>> {
        if !self.chapters {
            return None;
        }
        const PODCAST: &str = "/api/play-record/djradio/list";
        const BOOK: &str = "/api/play-record/voice/book/timeline/list";
        const PROGRAMS: &str = "/api/v6/dj/program/byradio";
        const AROUND: &str = "/api/dj/around/program/v1";
        const RADIO: &str = "/api/djradio/v3/get";
        const DETAIL: &str = "/api/dj/program/detail";
        const RESUME: &str = "/api/dj/user/setting/resumeplay/get";
        const RIGHTS: &str = "/api/dj/play/url/check";
        const SOURCE: &str = "/api/song/enhance/player/url/v1";
        const MEMBERSHIP: &str = "/api/djradio/get/subed";
        const SUBSCRIBE: &str = "/api/djradio/sub";
        const UNSUBSCRIBE: &str = "/api/djradio/unsub";
        const LIKED: &str = "/api/content/my/liked/voice";
        if !(matches!(
            path,
            PODCAST | BOOK | PROGRAMS | AROUND | RADIO | DETAIL | RESUME | RIGHTS | SOURCE
        ) || self.collections && matches!(path, MEMBERSHIP | SUBSCRIBE | UNSUBSCRIBE)
            || self.menu && path == LIKED)
        {
            return None;
        }
        self.chapter_calls
            .lock()
            .unwrap()
            .push(json!({"path":path,"args":args,"cached":cached}));
        let bad = || Error::Response("unexpected synthetic history chapter request");
        let result = (|| {
            let row = |id, name: &str, episode: Option<u64>, chapter: &str, kind: &str| {
                let mut data = json!({"id":id,"name":name,"resourceType":kind,"programCount":2,
                    "dj":{"nickname":"合成主播"},"voiceName":chapter,"tag":"续听"});
                if self.collections {
                    data["dj"]["userId"] = json!(if matches!(id, 83 | 84) { 2 } else { 7 });
                }
                if let Some(episode) = episode {
                    data["voiceId"] = json!(episode);
                }
                json!({"data":data,"playTime":1793180000000_u64-id,
                    "multiTerminalInfo":{"os":"windows","osText":"Windows"}})
            };
            let program = |radio, id, name: &str| {
                let mut data = json!({"id":id,"name":name,"mainSong":{"id":id+1000},
                    "duration":30000,"radio":{"id":radio,"name":match radio {
                        81 => "合成历史播客", 82 => "合成历史有声书", _ => "独立播放有声书",
                    },"categoryId":if radio==81 {3} else {10001}}});
                if self.collections {
                    data["radio"]["dj"] =
                        json!({"userId":if radio == 84 {2} else {7},"nickname":"合成主播"});
                }
                data
            };
            let number = |value: &Value| value.as_u64().or_else(|| value.as_str()?.parse().ok());
            if cached {
                return Err(bad());
            }
            Ok(match path {
                LIKED if self.menu && args == &json!({"limit":100,"likeTime":0}) => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                MEMBERSHIP if args == &json!({"targetUserId":"2","limit":1000}) => {
                    let ids = self.collected.lock().unwrap();
                    json!({"code":200,"hasMore":false,"count":ids.len(),
                        "djRadios":ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>()})
                }
                RADIO | SUBSCRIBE | UNSUBSCRIBE if self.collections => {
                    let id = args["id"]
                        .as_str()
                        .filter(|id| matches!(*id, "81" | "82"))
                        .ok_or_else(bad)?;
                    if args != &json!({"id":id}) {
                        return Err(bad());
                    }
                    let mut ids = self.collected.lock().unwrap();
                    match path {
                        RADIO => {
                            let name = if self.menu {
                                if id == "81" {
                                    "合成历史播客"
                                } else {
                                    "合成历史有声书"
                                }
                            } else {
                                "合成历史收藏"
                            };
                            json!({"code":200,"data":{"id":id,"name":name,"creator":{"userId":7},
                                "subed":ids.contains(id),"categoryId":if id=="81"{3}else{10001},
                                "programCount":2}})
                        }
                        SUBSCRIBE if ids.insert(id.to_owned()) => json!({"code":200}),
                        UNSUBSCRIBE if id == "81" && ids.remove(id) => {
                            return Err(Error::Transport(
                                "synthetic lost collection acknowledgement".into(),
                            ));
                        }
                        _ => return Err(bad()),
                    }
                }
                PODCAST if args == &json!({"filterVoiceBook":true}) => {
                    self.reads.fetch_add(1, Ordering::SeqCst);
                    let mut rows = vec![
                        row(81, "合成历史播客", Some(812), "播客第二集", "DJ_RADIO"),
                        row(83, "合成仅文本播客", None, "只有标题的单集", "DJ_RADIO"),
                    ];
                    self.filter_menu_removed("DJ_RADIO", &mut rows);
                    json!({"code":200,"data":{"list":rows}})
                }
                BOOK if args == &json!({}) => {
                    self.reads.fetch_add(1, Ordering::SeqCst);
                    let mut rows = vec![row(
                        82,
                        "合成历史有声书",
                        Some(822),
                        "有声书第二章",
                        "VOICE_BOOK",
                    )];
                    if self.collections {
                        rows.push(row(
                            84,
                            "独立播放有声书",
                            Some(842),
                            "独立播放第二章",
                            "VOICE_BOOK",
                        ));
                    }
                    self.filter_menu_removed("VOICE_BOOK", &mut rows);
                    json!({"code":200,"data":{"data":{
                        "todayRecords":rows,
                        "yesterdayRecords":[],"theDayBeforeYesterdayRecords":[],"earlierRecords":[]
                    }}})
                }
                PROGRAMS => {
                    let radio = number(&args["radioId"])
                        .filter(|id| matches!(id, 81 | 82) || (self.collections && *id == 84))
                        .ok_or_else(bad)?;
                    let ascending = args["asc"].as_bool();
                    let detail_request = self.menu
                        && ascending.is_some()
                        && args
                            == &json!({"radioId":radio.to_string(),"limit":200,"offset":0,"asc":ascending});
                    if !detail_request
                        && args != &json!({"radioId":radio.to_string(),"limit":250,"offset":0})
                    {
                        return Err(bad());
                    }
                    let names = match radio {
                        81 => ["播客第一集", "播客第二集"],
                        82 => ["有声书第一章", "有声书第二章"],
                        _ => ["独立播放第一章", "独立播放第二章"],
                    };
                    let mut programs = [
                        program(radio, radio * 10 + 1, names[0]),
                        program(radio, radio * 10 + 2, names[1]),
                    ];
                    if ascending == Some(false) {
                        programs.reverse();
                    }
                    json!({"code":200,"data":{"programs":programs,"count":2,"more":false,"asc":ascending}})
                }
                DETAIL
                    if matches!(args["id"].as_str(), Some("812" | "822"))
                        || self.menu && matches!(args["id"].as_str(), Some("811" | "821"))
                        || (self.collections && args["id"] == "842") =>
                {
                    let id = number(&args["id"]).unwrap();
                    let radio = id / 10;
                    let name = match (radio, id % 10) {
                        (81, 1) => "播客第一集",
                        (81, _) => "播客第二集",
                        (82, 1) => "有声书第一章",
                        (82, _) => "有声书第二章",
                        _ => "独立播放第二章",
                    };
                    json!({"code":200,"program":program(radio,id,name)})
                }
                RESUME
                    if matches!(args["radioId"].as_str(), Some("81" | "82"))
                        || (self.collections && args["radioId"] == "84") =>
                {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                RIGHTS
                    if matches!(args["voiceId"].as_str(), Some("812" | "822"))
                        || self.menu && matches!(args["voiceId"].as_str(), Some("811" | "821"))
                        || (self.collections && args["voiceId"] == "842") =>
                {
                    let id = number(&args["voiceId"]).unwrap();
                    json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":true}})
                }
                SOURCE => {
                    let ids: Vec<String> =
                        serde_json::from_str(args["ids"].as_str().ok_or_else(bad)?)?;
                    if ids.len() != 1
                        || !(matches!(ids[0].as_str(), "1812" | "1822")
                            || self.menu && matches!(ids[0].as_str(), "1811" | "1821")
                            || (self.collections && ids[0] == "1842"))
                    {
                        return Err(bad());
                    }
                    json!({"code":200,"data":[{"id":ids[0],
                        "url":"https://m1.music.126.net/cumulus-relay-fixture.wav",
                        "code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                _ => return Err(bad()),
            })
        })();
        Some(result)
    }

    fn filter_menu_removed(&self, kind: &str, rows: &mut Vec<Value>) {
        if self.menu
            && let Some(ids) = self.removed.lock().unwrap().get(kind)
        {
            rows.retain(|row| !ids.contains(&row["data"]["id"].to_string()));
        }
    }

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
    fn song_card_download_scope_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(self
            .song_download
            .as_ref()
            .map(|_| super::history_song_download_fixture::REVISION.to_owned()))
    }

    fn download_collection_scope_revision(&self) -> cumulus_core::Result<Option<String>> {
        Ok(self
            .song_collect
            .as_ref()
            .map(|_| super::history_song_collect_fixture::REVISION.to_owned())
            .or(self.song_card_download_scope_revision()?))
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, args) = write.into_request()?;
            if self.song_collect.is_none() && self.song_download.is_none() {
                return Err(Error::State("guarded playlist add is unavailable"));
            }
            permission()?;
            dispatched.store(true, Ordering::Release);
            if let Some(fixture) = &self.song_collect {
                fixture.add(path, args)
            } else {
                self.song_download
                    .as_ref()
                    .expect("guarded fixture")
                    .add(path, args)
            }
        })
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(result) = self
                .song_collect
                .as_ref()
                .and_then(|fixture| fixture.response(path, &args, cache))
                .or_else(|| {
                    self.song_download
                        .as_ref()
                        .and_then(|fixture| fixture.response(path, &args, cache))
                })
            {
                if path == "/api/play-record/song/list" {
                    self.reads.fetch_add(1, Ordering::SeqCst);
                }
                return result;
            }
            if let Some(result) = self
                .downloads
                .as_ref()
                .and_then(|fixture| fixture.response(path, &args, cache))
            {
                return result;
            }
            if let Some(result) = self.chapter_response(path, &args, cache) {
                return result;
            }
            if self.row_queue_menu
                && let Some(result) = self.row_menu.response(path, &args, cache)
            {
                return result;
            }
            let mode = self.mode.load(Ordering::SeqCst);
            if path.starts_with("/api/play-record/") {
                if path.ends_with("/delete") || path.ends_with("/clear") {
                    if self.row_queue_menu {
                        self.row_menu.authorize_delete(path, &args, cache)?;
                    }
                    if self.menu {
                        self.chapter_calls
                            .lock()
                            .unwrap()
                            .push(json!({"path":path,"args":args,"cached":cache}));
                    }
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
                self.reads.fetch_add(1, Ordering::SeqCst);
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
            if path == "/api/dj/user/setting/resumeplay/get" && args["radioId"] == "8" {
                return Ok(json!({"code":200,"data":{"resumePlayValue":0}}));
            }
            if matches!(path, "/api/dj/program/detail" | "/api/dj/play/url/check") {
                let key = if path.ends_with("/detail") {
                    "id"
                } else {
                    "voiceId"
                };
                let id = args[key]
                    .as_u64()
                    .or_else(|| args[key].as_str()?.parse().ok())
                    .filter(|id| (1..=100).contains(id))
                    .ok_or(Error::Response("unknown history fixture episode"))?;
                return Ok(if key == "id" {
                    json!({"code":200,"program":resource("VOICE",id)["pubDJProgramData"]})
                } else {
                    json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":true}})
                });
            }
            let mut reply = self.base.request(path, args, cache).await?;
            if self.row_queue_menu && path == "/api/song/enhance/privilege" {
                let rows = reply["data"]
                    .as_array_mut()
                    .ok_or(Error::Response("missing history fixture privileges"))?;
                for row in rows
                    .iter_mut()
                    .filter(|row| row["id"] == "8" || row["id"] == 8)
                {
                    row["st"] = json!(-100);
                    row["status"] = json!(-100);
                    row["pl"] = json!(0);
                }
            }
            Ok(reply)
        })
    }
}
