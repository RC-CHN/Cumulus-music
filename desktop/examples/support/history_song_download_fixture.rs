//! Closed history, playlist and media transport for the song download proof.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Mutex;

pub const REVISION: &str = "sealed-history-song-download";
const HISTORY: &str = "/api/play-record/song/list";
const PRIVILEGE: &str = "/api/song/enhance/privilege";
const LIKED: &str = "/api/song/like/get";
const DESTINATIONS: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const MEDIA_PREFIX: &str = "https://m1.music.126.net/cumulus-history-song-";
const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.mp3");
const MD5: &str = "485aa2633a203a9ea13fd7416caeb9f9";

#[derive(Default)]
struct State {
    calls: Vec<Value>,
    members: Vec<String>,
}

#[derive(Default)]
pub struct Fixture(Mutex<State>);

fn playlist(id: &str, owner: u64, name: &str, songs: &[String]) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner,"nickname":"合成创作者"},
        "name":name,"specialType":0,"privacy":if owner==2 {10}else{0},"tags":[],
        "trackCount":songs.len(),"trackIds":songs.iter().map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}

fn song(id: u64) -> Value {
    json!({"data":{"id":id,"name":format!("合成历史下载歌曲 · {id:02}"),
        "ar":[{"id":12,"name":"合成歌手"}],"al":{"id":13,"name":"合成历史下载专辑"},"dt":235000},
        "playTime":1793180000000_u64-id*1000,"multiTerminalInfo":{"os":"windows"}})
}

impl Fixture {
    pub fn report(&self, store: &StateStore) -> Value {
        let tasks = store.values("2", "download_tasks_v1").unwrap();
        let batch = store.get("2", "download_song_batch_v1", "last").unwrap();
        let order = batch
            .as_ref()
            .and_then(|value| value["tasks"].as_array())
            .into_iter()
            .flatten()
            .map(|id| {
                let task = &tasks[id.as_str().expect("download task receipt ID")];
                json!({"song":task["song_id"],"status":task["status"],"dispatch":task["dispatch"]})
            })
            .collect::<Vec<_>>();
        let state = self.0.lock().unwrap();
        json!({"calls":state.calls,"members":state.members,"batchOrder":order,"taskCount":tasks.len()})
    }

    pub fn response(&self, path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
        if !matches!(
            path,
            HISTORY
                | PRIVILEGE
                | LIKED
                | DESTINATIONS
                | DETAIL
                | AUTH
                | ADD
                | "/api/playlist/create"
                | "/api/song/like"
        ) {
            return None;
        }
        if path == DETAIL && !matches!(args["id"].as_str(), Some("81" | "91")) {
            return None;
        }
        let mut state = self.0.lock().unwrap();
        let bad = || Error::Response("unexpected history song download fixture request");
        Some((|| {
            if cached && path != LIKED {
                return Err(bad());
            }
            Ok(match path {
                HISTORY if args == &json!({"limit":300}) => {
                    json!({"code":200,"data":{"list":(1..=4).map(song).collect::<Vec<_>>()}})
                }
                PRIVILEGE => {
                    let ids: Vec<String> =
                        serde_json::from_str(args["ids"].as_str().ok_or_else(bad)?)?;
                    if args.as_object().is_none_or(|values| values.len() != 1)
                        || ids
                            .iter()
                            .any(|id| !matches!(id.as_str(), "1" | "2" | "3" | "4"))
                    {
                        return Err(bad());
                    }
                    json!({"code":200,"data":ids.iter().map(|id|json!({"id":id,"fee":0,"subp":1,
                        "st":if id=="1" {-100}else{0},"dl":if id=="1" {0}else{128000},"pl":128000}))
                        .collect::<Vec<_>>()})
                }
                LIKED if args == &json!({"uid":"2"}) => {
                    // Both survivors are liked. Only keeping the original
                    // downed first row as anchor can open the picker.
                    state
                        .calls
                        .push(json!({"kind":"liked","ids":["2","3","4"]}));
                    json!({"code":200,"ids":[2,3,4]})
                }
                DESTINATIONS
                    if matches!(args["sortType"].as_str(), Some("default" | "recently"))
                        && args
                            == &json!({"uid":"2","offset":0,"limit":10,"sortType":args["sortType"]}) =>
                {
                    state.calls.push(json!({"kind":"destinations","args":args}));
                    json!({"code":200,"more":false,"playlist":[
                        playlist("81",2,"合成下载目的地",&state.members),
                        playlist("91",7,"不属于当前账号的歌单",&[])]})
                }
                DETAIL => {
                    let id = args["id"].as_str().ok_or_else(bad)?;
                    if args != &json!({"id":id,"n":100000,"s":0}) {
                        return Err(bad());
                    }
                    let row = if id == "81" {
                        playlist(id, 2, "合成下载目的地", &state.members)
                    } else {
                        playlist(id, 7, "不属于当前账号的歌单", &[])
                    };
                    state
                        .calls
                        .push(json!({"kind":"detail","pid":id,"trackIds":row["trackIds"]}));
                    json!({"code":200,"playlist":row})
                }
                AUTH => {
                    state.calls.push(json!({"kind":"authorize","args":args}));
                    let id = args["id"].as_str().ok_or_else(bad)?;
                    if !matches!(id, "2" | "3" | "4")
                        || args != &json!({"id":id,"level":"standard","immerseType":"c51"})
                    {
                        return Err(bad());
                    }
                    json!({"code":200,"data":{"id":id,"code":200,
                        "url":format!("{MEDIA_PREFIX}{id}.mp3"),"type":"mp3",
                        "md5":MD5,"size":AUDIO.len(),"level":"standard"}})
                }
                ADD | "/api/playlist/create" | "/api/song/like" => {
                    state
                        .calls
                        .push(json!({"kind":"unguarded_write","path":path,"args":args}));
                    return Err(bad());
                }
                _ => return Err(bad()),
            })
        })())
    }

    pub fn add(&self, path: &str, args: Value) -> Result<Value> {
        let mut state = self.0.lock().unwrap();
        state
            .calls
            .push(json!({"kind":"guarded_add","path":path,"args":args}));
        if path != ADD
            || args != json!({"pid":"81","op":"add","trackIds":r#"["1","2","3"]"#})
            || !state.members.is_empty()
        {
            return Err(Error::Response("unexpected history song download add"));
        }
        state.members = ["1", "2", "3"].into_iter().map(str::to_owned).collect();
        Ok(json!({"code":200}))
    }
}

impl MediaBackend for Fixture {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let id = request
                .url
                .as_str()
                .strip_prefix(MEDIA_PREFIX)
                .and_then(|path| path.strip_suffix(".mp3"))
                .filter(|id| matches!(*id, "2" | "3" | "4"))
                .ok_or(Error::State("unexpected synthetic song media"))?;
            if request.offset != 0 {
                return Err(Error::State("unexpected synthetic song transfer"));
            }
            self.0
                .lock()
                .unwrap()
                .calls
                .push(json!({"kind":"transfer","id":id}));
            Ok(MediaReply {
                status: 200,
                headers: [("content-length".into(), AUDIO.len().to_string())].into(),
                body: Box::new(Body(AUDIO)),
            })
        })
    }
}

struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let len = self.0.len().min(buffer.len());
            buffer[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        })
    }
}
