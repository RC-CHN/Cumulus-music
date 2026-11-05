//! Closed synthetic transport for the classic podcast home and Settings proof.
use cumulus_core::{
    Error, Result,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const EXPERIMENT: &str = "/api/rtrs/abt/front/expinfo/list";
const DIRECTORY: &str = "/api/voice/homepage/block/content";
const MEDIA: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";

pub struct SceneHome {
    pub case: String,
    reads: AtomicUsize,
    calls: Mutex<Vec<Value>>,
}
impl SceneHome {
    pub fn new(case: String) -> Self {
        assert!(matches!(
            case.as_str(),
            "automatic" | "manual" | "updated" | "retry"
        ));
        Self {
            case,
            reads: AtomicUsize::new(0),
            calls: Mutex::default(),
        }
    }

    pub fn calls(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }

    pub fn response(&self, path: &str, body: &Value) -> Option<Result<Value>> {
        let top = path == EXPERIMENT && *body == json!({"expNames":"podcast_tab_better"});
        if top
            || matches!(
                path,
                DIRECTORY
                    | "/api/content/scene/fm/play/list"
                    | "/api/dj/program/detail"
                    | "/api/dj/play/url/check"
                    | "/api/song/enhance/player/url/v1"
                    | "/api/dj/user/setting/resumeplay/get"
            )
        {
            self.calls.lock().unwrap().push(json!([path, body]));
        }
        Some(Ok(match path {
            EXPERIMENT if top => json!({"code":200,"data":[{
                "expName":"podcast_tab_better",
                "expGroupName":if self.case == "updated" { "t1" } else { "c" }
            }]}),
            DIRECTORY => {
                assert_eq!(*body, json!({"blockCode":"VOICE_HOMEPAGE_BLOCK_SCENE_FM"}));
                if self.reads.fetch_add(1, Ordering::SeqCst) == 0 && self.case == "retry" {
                    return Some(Err(Error::Transport(
                        "synthetic directory unavailable".into(),
                    )));
                }
                scene_directory()
            }
            "/api/content/scene/fm/play/list" => {
                return Some(Err(Error::Response("unexpected lazy Scene FM list read")));
            }
            "/api/dj/program/detail" => {
                let id = fixture_id(&body["id"], 601, 602);
                json!({"code":200,"program":voice(id)})
            }
            "/api/dj/play/url/check" => {
                let id = fixture_id(&body["voiceId"], 601, 602);
                json!({"code":200,"data":{"voiceId":id,"songId":1000+id,"canPlay":true}})
            }
            "/api/song/enhance/player/url/v1" => {
                let ids: Vec<Value> =
                    serde_json::from_str(body["ids"].as_str().expect("fixture player IDs"))
                        .unwrap();
                assert_eq!(ids.len(), 1);
                fixture_id(&ids[0], 1601, 1602);
                assert_eq!(body["level"], "standard");
                json!({"code":200,"data":[{"id":ids[0],"url":MEDIA,"code":200,"md5":"fixture","level":"standard","expi":120}]})
            }
            "/api/dj/user/setting/resumeplay/get" => {
                json!({"code":200,"data":{"resumePlayValue":0}})
            }
            "/api/search/default/keyword/get" => json!({"code":200,"data":{
                "refreshTime":60000,"keywords":[{"realkeyword":"合成关键词","showKeyword":"发现合成声音"}]
            }}),
            "/api/search/pc/rcmd/keyword/get" => {
                json!({"code":200,"data":{"operateWords":{},"algWords":[]}})
            }
            "/api/search/pc/chart/list" => json!({"code":200,"data":{
                "tabInfo":{"list":[{"id":"HOT_SEARCH_DJ#@#","name":"播客榜","button":{"text":"更多"}}]},
                "items":{"HOT_SEARCH_DJ#@#":[{"toUserWord":"合成播客条目","url":"orpheus://audio/detail?id=80"}]}
            }}),
            "/api/dj/radio/category/list" => json!({"code":200,"data":{"parentCategoryVoList":[]}}),
            "/api/voice/homepage/block/page" => {
                json!({"code":200,"data":{"blocks":[],"cursor":null,"hasMore":false}})
            }
            "/api/charts/voicelist" | "/api/charts/voice" | "/api/voice/book/chart/get" => {
                json!({"code":200,"data":[]})
            }
            "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
            "/api/link/position/show/resource" => {
                json!({"code":200,"data":{"commonResourceList":[]}})
            }
            _ => return None,
        }))
    }
}

// Reuse only the synthetic response shape for the older Classic fallback proof.
pub fn scene_directory() -> Value {
    json!({"code":200,"data":[{"creatives":[{
        "creativeId":"OP_1",
        "uiElement":{"mainTitle":{"title":"合成场景"},"labelTexts":["可离线验证"]},
        "creativeExtInfoVO":{"djPrograms":[voice(601),voice(602)]}
    },{
        "creativeId":"OP_7",
        "uiElement":{"mainTitle":{"title":"另一合成场景"}}
    }]}]})
}

fn voice(id: u64) -> Value {
    json!({"id":id,"name":format!("合成场景单集 {id}"),"mainSong":{"id":1000+id,"duration":30000},
        "radio":{"id":80,"name":"合成场景播客"},"duration":30000})
}

fn fixture_id(value: &Value, first: u64, second: u64) -> u64 {
    let id = value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .expect("synthetic Scene FM identifier");
    assert!(id == first || id == second, "unknown synthetic Scene FM ID");
    id
}

pub fn baseline<'a>(
    api: &'a super::settings_fixture::Library,
    path: &'a str,
    body: Value,
    cache: bool,
) -> ApiFuture<'a, Value> {
    // These are synthetic startup/settings reads; do not pass possible writes
    // into a shared fixture with a successful settings mutation implementation.
    if matches!(
        path,
        "/api/w/nuser/account/get"
            | "/api/user/setting"
            | "/api/user/playlist"
            | "/api/song/like/get"
            | "/api/v3/discovery/recommend/songs"
            | "/api/v3/song/detail"
            | "/api/rtrs/abt/front/expinfo/list"
    ) {
        return api.request(path, body, cache);
    }
    Box::pin(async move { Err(Error::Response("unsupported synthetic Scene FM request")) })
}
