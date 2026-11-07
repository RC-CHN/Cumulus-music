//! Sealed PC comprehensive search, two result blocks, and real null audio.
use cumulus_core::{Error, Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const QUERY: &str = "合成播单续播";

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_PODCAST_PLAYBACK_PROOF").is_some()
}

pub fn initialize(store: &StateStore) -> Result<()> {
    if enabled() {
        store.put("2", "podcast-radio-order-v1", "7", &json!(false))?;
        store.put(
            "2",
            "podcast-latest-voice-v1",
            "7",
            &json!({
                "id":"1","mainTrackId":"1001","name":"合成节目 1","time":19,"duration":30
            }),
        )?;
    }
    Ok(())
}

pub fn report() -> Value {
    json!(CALLS.lock().unwrap().as_slice())
}

fn episode(id: u64) -> Value {
    json!({
        "id":id,"name":format!("合成节目 {id}"),"duration":30000,
        "mainSong":{"id":id+1000,"duration":30000},
        "radio":{"id":7,"name":"合成续播播单","categoryId":2}
    })
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    match path {
        "/api/search/pc/result/tab"
        | "/api/search/pc/complex/page"
        | "/api/search/pc/complex/page/v3"
        | "/api/v6/dj/program/byradio"
        | "/api/dj/program/detail"
        | "/api/dj/play/url/check"
        | "/api/song/enhance/player/url/v1"
        | "/api/djradio/v3/get"
        | "/api/dj/around/program/v1"
        | "/api/dj/user/setting/resumeplay/get" => {}
        _ => return None,
    }
    CALLS
        .lock()
        .unwrap()
        .push(json!({"path":path,"args":args,"cached":cached}));
    let response = match path {
        "/api/search/pc/result/tab" => {
            assert_eq!(*args, json!({"keyword":QUERY}));
            json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"播单"}]}})
        }
        "/api/search/pc/complex/page" | "/api/search/pc/complex/page/v3" => {
            assert_eq!(args["keyword"], QUERY);
            let v3 = path.ends_with("/v3");
            assert_eq!(v3, super::search_fixture::v3_enabled());
            let radio = |id: u64, name: &str, count: u64| {
                let row = json!({"id":id,"name":name,"categoryId":2,"programCount":count});
                let base = if v3 {
                    json!({"pubDJRadioData":row})
                } else {
                    row
                };
                json!({"type":"voicelist","baseInfo":base})
            };
            let best = if v3 {
                "search_block_hot_result_core_resources"
            } else {
                "search_block_best_match"
            };
            let allowed = radio(7, "合成续播播单", 251);
            json!({"code":200,"data":{"blocks":[
                {"blockCode":best,"resources":[allowed.clone()]},
                {"blockCode":"search_block_voicelist","uiElement":{"mainTitle":{"title":"播客"}},
                    "resources":[allowed,radio(8,"合成不可加载播单",1)]}
            ]}})
        }
        "/api/v6/dj/program/byradio" if args["radioId"] == "8" => {
            assert_eq!(*args, json!({"radioId":"8","offset":0,"limit":250}));
            return Some(Err(Error::Transport("合成播单读取失败".into())));
        }
        "/api/v6/dj/program/byradio" => {
            let offset = args["offset"].as_u64().unwrap() as usize;
            assert!(matches!(offset, 0 | 250));
            assert_eq!(
                *args,
                json!({"radioId":"7","offset":offset,"limit":250,"asc":false})
            );
            json!({"code":200,"data":{
                "programs":(1..=251).rev().skip(offset).take(250).map(episode).collect::<Vec<_>>(),
                "count":251,"more":offset==0,"asc":false
            }})
        }
        "/api/dj/program/detail" => {
            assert_eq!(*args, json!({"id":"1"}));
            json!({"code":200,"program":episode(1)})
        }
        "/api/dj/play/url/check" => {
            assert_eq!(args["voiceId"], "1");
            json!({"code":200,"data":{"voiceId":1,"songId":1001,"canPlay":true}})
        }
        "/api/song/enhance/player/url/v1" => {
            let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
            assert_eq!(ids.len(), 1);
            assert!(ids[0] == "1001" || ids[0] == 1001);
            json!({"code":200,"data":[{"id":1001,"code":200,"level":"standard","expi":120,
                "url":"https://m1.music.126.net/cumulus-relay-fixture.wav","md5":"fixture"}]})
        }
        _ => panic!("unexpected search podcast route {path}"),
    };
    Some(Ok(response))
}
