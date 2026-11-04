use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const PLAYLISTS: &str = "/api/playlist/category/list";
const SONGS: &str = "/api/personalized/newsong";
const BOOKS: &str = "/api/podcast/common/single/block/get";
const VOICES: &str = "/api/pc/voicelist/rcmd/list";

fn actions_enabled() -> bool {
    std::env::var_os("CUMULUS_ORDINARY_ESSENTIAL_ACTIONS_PROOF").is_some()
}

fn voice_actions_enabled() -> bool {
    std::env::var_os("CUMULUS_ORDINARY_ESSENTIAL_VOICE_ACTIONS_PROOF").is_some()
}

pub struct Library {
    base: Arc<dyn Api>,
    settings: Mutex<(u64, String)>,
    reads: Mutex<Vec<Value>>,
}
impl Library {
    pub fn new(base: Arc<dyn Api>) -> Self {
        Self {
            base,
            settings: Mutex::new((2, "initial".into())),
            reads: Mutex::default(),
        }
    }
    pub fn set(&self, owner: u64, mode: &str) {
        assert!(matches!(owner, 0 | 2 | 3));
        assert!(matches!(
            mode,
            "initial" | "all-down" | "songs-down" | "books-down" | "detail-down"
        ));
        *self.settings.lock().unwrap() = (owner, mode.into());
    }
    pub fn report(&self) -> Value {
        json!(self.reads.lock().unwrap().clone())
    }
}

fn playlist(id: u64) -> Value {
    json!({"id":id,"name":format!("合成官方歌单 {id}"),"userId":2,
        "creator":{"userId":2,"nickname":"合成用户"},"trackCount":0,"privacy":0,"tracks":[],"trackIds":[]})
}

fn action_playlist(id: u64, preview: bool) -> Value {
    let mut result = playlist(id);
    let total = match id {
        501 => 105,
        502 => 0,
        _ => 2,
    };
    result["userId"] = json!(9);
    result["creator"]["userId"] = json!(9);
    result["trackCount"] = json!(total);
    if !preview {
        result["trackIds"] = json!(
            (1000..1000 + total)
                .map(|id| json!({"id":id}))
                .collect::<Vec<_>>()
        );
    } else {
        let row = result.as_object_mut().unwrap();
        row.remove("tracks");
        row.remove("trackIds");
        if id == 504 {
            row.remove("trackCount");
        }
    }
    if id == 503 {
        result["privacy"] = json!(10);
    } else if id == 506 && preview {
        result.as_object_mut().unwrap().remove("privacy");
    }
    result
}

fn voice_media_id(id: u64) -> u64 {
    if voice_actions_enabled() && id == 701 {
        2 // The selected catalog song and the episode share a backing media ID.
    } else {
        id + 1000
    }
}

fn voice(id: u64, radio: u64) -> Value {
    json!({"id":id,"name":format!("合成声音 {id}"),"duration":25000,"mainSong":{"id":voice_media_id(id),"name":format!("合成声音 {id}"),"duration":25000},
        "radio":{"id":radio,"name":format!("合成播单 {radio}"),"programCount":2},"dj":{"brand":format!("合成播单 {radio}")}})
}

fn number(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .unwrap_or(0)
}

impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (owner, mode) = self.settings.lock().unwrap().clone();
            let selected = matches!(path, PLAYLISTS | SONGS | BOOKS | VOICES);
            if selected {
                self.reads
                    .lock()
                    .unwrap()
                    .push(json!({"owner":owner,"mode":mode,"path":path,"args":args}));
                assert_ne!(owner, 0, "choice business must not run on a guest account");
                if mode == "all-down"
                    || (mode == "songs-down" && path == SONGS)
                    || (mode == "books-down" && path == BOOKS)
                {
                    return Err(Error::Transport("synthetic choice block failure".into()));
                }
            }
            let actions = actions_enabled();
            if actions && matches!(path, "/api/v6/playlist/detail" | "/api/v3/song/detail") {
                self.reads
                    .lock()
                    .unwrap()
                    .push(json!({"owner":owner,"mode":mode,"path":path,"args":args}));
                if path == "/api/v6/playlist/detail" && mode == "detail-down" {
                    return Err(Error::Transport("synthetic playlist read failure".into()));
                }
            }
            if voice_actions_enabled()
                && matches!(
                    path,
                    "/api/dj/program/detail"
                        | "/api/dj/play/url/check"
                        | "/api/dj/around/program/v1"
                        | "/api/v6/dj/program/byradio"
                        | "/api/song/enhance/player/url/v1"
                )
            {
                self.reads
                    .lock()
                    .unwrap()
                    .push(json!({"owner":owner,"mode":mode,"path":path,"args":args}));
            }
            Ok(match path {
                "/api/w/nuser/account/get" if owner == 0 => json!({"code":200,"profile":null}),
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":owner,"nickname":format!("精选账号 {owner}")}})
                }
                "/api/user/playlist" if actions => {
                    json!({"code":200,"more":false,"playlist":[action_playlist(505,true)]})
                }
                "/api/playlist/subscribe" | "/api/playlist/unsubscribe" if actions => {
                    panic!("proof may open but never confirm account collection mutations");
                }
                PLAYLISTS => {
                    assert_eq!(args["cat"], "官方");
                    assert_eq!(args["newStyle"], true);
                    let list = (501..=506)
                        .map(|id| {
                            if actions {
                                action_playlist(id, true)
                            } else {
                                playlist(id)
                            }
                        })
                        .collect::<Vec<_>>();
                    json!({"code":200,"playlists":list,"playlistIds":(501..=506).collect::<Vec<_>>()})
                }
                SONGS => {
                    assert_eq!(args, json!({"limit":12,"areaId":0}));
                    json!({"code":200,"result":(1..=10).map(|id|json!({
                        "song":{"id":id,"name":format!("精选新歌 {owner} / {id}"),"artists":[{"id":12,"name":"合成歌手"}]},
                        "alg":"SYNTHETIC_PRIVATE","signedUrl":"https://blocked.invalid/secret"
                    })).collect::<Vec<_>>()})
                }
                BOOKS => {
                    assert_eq!(args["pageCode"], "PC_HOMEPAGE_PODCAST");
                    assert_eq!(args["blockCode"], "PC_HOMEPAGE_VOICEBOOK_RCMD");
                    let extra: Value = serde_json::from_str(args["extInfo"].as_str().unwrap())?;
                    assert_eq!(extra["position"], "essential");
                    let category = extra["interestTypes"][0]["resourceId"]
                        .as_str()
                        .unwrap_or("type0");
                    let mut books = (601..=608).map(|id|json!({
                        "creativeId":id,"uiElement":{"mainTitle":{"title":format!("{category} 合成有声书 {id}")}},
                        "creativeExtInfoVO":{"radio":{"id":id,"name":format!("合成播单 {id}")}},
                    })).collect::<Vec<_>>();
                    books.insert(
                        0,
                        json!({"uiElement":{"mainTitle":{"title":"听过的有声书"}}}),
                    );
                    json!({"code":200,"data":{"blockVOS":[{"uiElement":{"mainTitle":{"title":"精选有声书"}},"creatives":books}]}})
                }
                VOICES => {
                    assert_eq!(args, json!({"limit":12}));
                    json!({"code":200,"data":{"recommendVoiceVOS":(701..=710).map(|id|json!({
                        "djProgram":voice(id, 600), "alg":"SYNTHETIC_PRIVATE"
                    })).collect::<Vec<_>>()}})
                }
                "/api/v6/playlist/detail" if actions => {
                    let id = number(&args["id"]);
                    assert!((501..=506).contains(&id));
                    assert_eq!(args["n"], 100000);
                    assert_eq!(args["s"], 0);
                    json!({"code":200,"playlist":action_playlist(id,false),"privileges":[]})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":playlist(number(&args["id"])),"privileges":[]})
                }
                "/api/djradio/v3/get" => {
                    json!({"code":200,"data":{"id":args["id"],"name":format!("合成播单 {}",args["id"].as_str().unwrap_or("")),"programCount":2}})
                }
                "/api/v6/dj/program/byradio" => {
                    json!({"code":200,"data":{"programs":[voice(701, number(&args["radioId"])),voice(702, number(&args["radioId"]))],"count":2,"more":false,"asc":args["asc"]}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/dj/program/detail" => {
                    json!({"code":200,"program":voice(number(&args["id"]),600)})
                }
                "/api/dj/play/url/check" => {
                    json!({"code":200,"data":{"voiceId":args["voiceId"],"songId":voice_media_id(number(&args["voiceId"])),"canPlay":true}})
                }
                "/api/djradio/get/subed" => {
                    json!({"code":200,"djRadios":[],"count":0,"hasMore":false})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/voice/book/recent" => json!({"code":200,"data":{"djPrograms":[]}}),
                "/api/play-record/voice/book/timeline/list" => json!({"code":200,"data":{"data":{
                    "todayRecords":[],"yesterdayRecords":[],"theDayBeforeYesterdayRecords":[],"earlierRecords":[]
                }}}),
                "/api/v2/discovery/new/songs" => json!({"code":200,"data":[]}),
                "/api/ad/sdk/pc/visible" | "/api/v2/banner/get" => {
                    panic!("choice proof must never invoke ad/banner services")
                }
                _ => return self.base.request(path, args, cache).await,
            })
        })
    }
}
