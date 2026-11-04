use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::Mutex;

const FEED: &str = "/api/event/pc/history/feed/get";
const DETAIL: &str = "/api/v1/event/73/11";
const UNAVAILABLE: &str = "/api/v1/event/73/15";
const COMMENTS: &str = "/api/v1/resource/comments/A_EV_2_11_73";
const CURSOR: &str = "90071992547409933331";
const NOTICE: &str = "/api/user/friend/new/song/notice/web/get";
const RELEASES: &str = "/api/sub/artist/new/works/song-mv/list/v2";
const LATEST: &str = "/api/sub/artist/new/works/song/playall";
const COMPOSER: [&str; 5] = [
    "/api/search/song/list/page",
    "/api/v1/search/artist/get",
    "/api/v1/search/album/get",
    "/api/v1/search/playlist/get",
    "/api/search/voicelist/get",
];

fn composer(path: &str, body: &Value, attempt: usize) -> Value {
    let kind = COMPOSER
        .iter()
        .position(|candidate| *candidate == path)
        .unwrap();
    let offset = body["offset"].as_u64().unwrap();
    let expected = match kind {
        0 => {
            json!({"keyword":"合成附件","scene":"normal","limit":8,"offset":offset,"needCorrect":true})
        }
        4 => json!({"keyword":"合成附件","scene":"normal","limit":8,"offset":offset}),
        _ => json!({"s":"合成附件","limit":8,"offset":offset,"queryCorrect":true}),
    };
    assert_eq!(*body, expected);
    let item = |id, name: String| json!({"id":id,"name":name,"sensitive":"SYNTHETIC_PRIVATE"});
    match kind {
        0 if offset == 0 => {
            let rows = (1..=8).map(|id| json!({"baseInfo":{"simpleSongData":item(id, format!("合成附件单曲 {id}"))}})).collect::<Vec<_>>();
            json!({"code":200,"data":{"resources":rows}})
        }
        0 if offset == 8 && attempt == 1 => json!({"code":503}),
        0 if offset == 8 => {
            json!({"code":200,"data":{"resources":[{"baseInfo":{"simpleSongData":item(9,"合成附件单曲续页".into())}}]}})
        }
        1 if offset == 0 => {
            json!({"code":200,"result":{"artists":[item(91,"合成附件歌手".into())]}})
        }
        2 if offset == 0 => {
            json!({"code":200,"result":{"albums":[item(92,"合成附件专辑".into())]}})
        }
        3 if offset == 0 => {
            json!({"code":200,"result":{"playlists":[item(93,"合成附件歌单".into())]}})
        }
        4 if offset == 0 => {
            json!({"code":200,"data":{"resources":[{"baseInfo":item(94,"合成附件播客".into())}]}})
        }
        _ => panic!("unexpected composer fixture offset"),
    }
}

#[derive(Default)]
pub struct Library {
    calls: Mutex<Vec<(String, Value)>>,
    fallback: super::catalog_fixture::Library,
    profile_notes: super::profile_notes_fixture::Library,
}
impl Library {
    pub fn block_profile_notes(&self) {
        self.profile_notes.block();
    }

    pub fn report(&self) -> Value {
        let calls = self.calls.lock().unwrap();
        let mut report = json!({"feed": calls.iter().filter(|(path, _)|path==FEED).map(|(_,body)|body).collect::<Vec<_>>(),
            "notice": calls.iter().filter(|(path, _)|path==NOTICE).map(|(_,body)|body).collect::<Vec<_>>(),
            "releaseReads": calls.iter().filter(|(path, _)|path==RELEASES||path==LATEST).count(),
            "composer": calls.iter().filter(|(path, _)|COMPOSER.contains(&path.as_str())).collect::<Vec<_>>(),
            "details":calls.iter().filter(|(path, _)|path==DETAIL).count(),
            "unavailable":calls.iter().filter(|(path, _)|path==UNAVAILABLE).count(),
            "commentOffsets":calls.iter().filter(|(path, _)|path==COMMENTS).map(|(_,body)|&body["offset"]).collect::<Vec<_>>()});
        if super::profile_notes_fixture::Library::enabled() {
            report["profileNotesCalls"] = self.profile_notes.report();
        }
        report
    }
}
fn row(id: u64, label: &str, anonymous: bool) -> Value {
    json!({"id":id,"type":if id==14 {21} else {19},
        "eventTime":1700000000000_u64,"user":{"userId":73,
        "nickname":format!("合成作者 {id}"),"signature":"SYNTHETIC_PRIVATE"},
        "anonymityInfo":if anonymous {Some(json!({"encryptUserId":"SYNTHETIC_PRIVATE"}))} else {None},
        "info":{"commentCount":if id==11 {3} else {0},"likedCount":5,"shareCount":1,"threadId":format!("A_EV_2_{id}_73")},
        "pics":[{"originUrl":"https://invalid.example/SYNTHETIC_PRIVATE"}],
        "json":json!({"title":label,"msg":"这是合成动态，不涉及任何真实账号。",
            "album":if id==11 {Some(json!({"id":7,"name":"合成动态专辑"}))} else {None}}).to_string()})
}
fn comment(id: u64, label: &str) -> Value {
    json!({"commentId":id,"likedCount":0,"content":label,"time":1700000000001_u64,
        "user":{"userId":91,"nickname":"合成评论作者","signature":"SYNTHETIC_PRIVATE"}})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(result) = self.profile_notes.request(path, &payload, cache) {
                return result;
            }
            if ![
                FEED,
                DETAIL,
                UNAVAILABLE,
                COMMENTS,
                NOTICE,
                RELEASES,
                LATEST,
            ]
            .contains(&path)
                && !COMPOSER.contains(&path)
            {
                // Closed catalogue fixture; it has no network implementation.
                return self.fallback.request(path, payload, cache).await;
            }
            let attempt = {
                let mut calls = self.calls.lock().unwrap();
                calls.push((path.into(), payload.clone()));
                calls
                    .iter()
                    .filter(|(p, body)| p == path && *body == payload)
                    .count()
            };
            assert!(!cache);
            if COMPOSER.contains(&path) {
                return Ok(composer(path, &payload, attempt));
            }
            Ok(match path {
                NOTICE if payload == json!({}) => json!({"code":200,"data":{
                    "style":"all-normal","time":1700000000000_u64,"text":"合成关注歌手发布了新歌"}}),
                NOTICE
                    if payload.as_object().is_some_and(|object| object.len() == 1)
                        && payload["time"]
                            .as_u64()
                            .is_some_and(|time| time > 1700000000000) =>
                {
                    json!({"code":200,"data":{"style":"musician-empty"}})
                }
                RELEASES if payload["firstRequest"] == true && payload["sourceType"] == 1 => {
                    json!({"code":200,"data":{"newWorks":[],"hasMore":false}})
                }
                LATEST if payload == json!({}) => json!({"code":200,"data":{"songList":[]}}),
                FEED if payload == json!({"cursor":"-1"}) => {
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        json!({"code":200,"data":{"cursor":CURSOR,"more":true,"events":[
                            row(11,"合成首条",false), row(12,"合成普通动态",false), row(13,"合成待匿名",false),
                            row(14,"不得显示的类型",false), row(15,"合成已不可见",false)
                        ]}})
                    }
                }
                FEED if payload == json!({"cursor":CURSOR}) => {
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        json!({"code":200,"data":{"cursor":0,"more":false,"events":[
                            row(13,"合成匿名动态",true), row(16,"合成第二页",false)
                        ]}})
                    }
                }
                FEED if payload == json!({"cursor":-1}) => {
                    json!({"code":200,"data":{"cursor":0,"more":false,"events":[]}})
                }
                DETAIL if payload == json!({}) => {
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        json!({"code":200,"event":row(11,"合成详情正文",false)})
                    }
                }
                UNAVAILABLE if payload == json!({}) => json!({"code":404}),
                COMMENTS => {
                    let offset = payload["offset"].as_u64().unwrap();
                    assert_eq!(
                        payload,
                        json!({"limit":60,"offset":offset,"beforeTime":0,
                        "compareUserLocation":false,"commentId":"0","composeConcert":false,
                        "markReplied":false,"forceFlatComment":false,"showInner":false})
                    );
                    match offset {
                        0 if attempt == 1 => json!({"code":503}),
                        0 => json!({"code":200,"total":3,"more":true,
                            "comments":[comment(41,"合成评论一"),comment(42,"合成评论二")],
                            "hotComments":[comment(44,"合成精彩评论")]}),
                        2 => {
                            json!({"code":200,"total":3,"more":false,"comments":[comment(43,"合成评论续页")]})
                        }
                        _ => {
                            return Err(Error::Response(
                                "unexpected community fixture comment offset",
                            ));
                        }
                    }
                }
                _ => return Err(Error::Response("unexpected community fixture request")),
            })
        })
    }
}
