//! Sealed, read-only inline-credit search and detail navigation.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const SONGS: &str = "/api/search/song/list/page";
const LYRICS: &str = "/api/search/resource/lyric";
const ARTIST: &str = "/api/artist/v3/detail";
const ALBUM: &str = "/api/album/v3/detail";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub(super) fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

fn artist() -> Value {
    json!({"id":31402,"name":"主艺人","trans":"合成译艺人","alias":"合成备选艺名","musicSize":0})
}

fn album() -> Value {
    json!({"id":31403,"name":"主专辑","tns":["合成译专辑"],"size":1,"artists":[artist()]})
}

fn song() -> Value {
    json!({
        "id":31401,"name":"主曲名","dt":186000,
        "tns":["未命中译名","合成译名"],"alia":["合成不内联别名"],
        "ar":[artist(), {"name":"无链接艺人","alias":["合成不显示艺人别名"]}],
        "al":album(),
        "privilege":{"id":31401,"fee":0,"st":0,"pl":320000,"dl":320000}
    })
}

fn negative() -> Value {
    json!({
        "id":31404,"name":"只有别名的歌曲","dt":187000,
        "alia":["合成不内联别名"],"ar":[{"name":"无链接艺人","alias":["合成不显示艺人别名"]}],
        "al":{"name":"无链接专辑","tns":["合成无链接译专辑"]},
        "privilege":{"id":31404,"fee":0,"st":0,"pl":320000,"dl":320000}
    })
}

pub(super) fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    let response = match path {
        SONGS | LYRICS => {
            if args["keyword"] != "合成" || args["offset"] != 0 || args["scene"] != "normal" {
                return Some(Err(Error::Response("unexpected inline credit search")));
            }
            let resources = if path == SONGS {
                json!([
                    {"type":"song","baseInfo":{"simpleSongData":song()},
                     "extInfo":{"lyrics":{"txt":"题合成词","range":[{"first":1,"second":3}]}}},
                    {"type":"song","baseInfo":{"simpleSongData":negative()}}
                ])
            } else {
                json!([
                    {"song":song(),"tns":["合成外层译名"],"lyrics":"合成歌词保持原文\n第二行"},
                    {"song":negative(),"lyrics":"负例也保留原文"}
                ])
            };
            json!({"code":200,"data":{"resources":resources,"totalCount":2,"hasMore":false}})
        }
        ARTIST if *args == json!({"id":"31402","top":50,"e_r":false}) => {
            json!({"code":200,"artist":artist(),"hotSongs":[]})
        }
        "/api/artist/detail/dynamic" if *args == json!({"id":"31402"}) => {
            json!({"code":200,"followed":false})
        }
        ALBUM if *args == json!({"id":"31403"}) => {
            json!({"code":200,"album":album(),"songs":[song()]})
        }
        "/api/album/detail/dynamic" if *args == json!({"id":"31403"}) => {
            json!({"code":200,"isSub":false})
        }
        "/api/album/privilege" if *args == json!({"id":"31403"}) => {
            json!({"code":200,"data":[{"id":31401,"st":0,"pl":320000}]})
        }
        "/api/resource-exposure/config" if args["resourceId"] == "31403" => {
            json!({"code":200,"data":{"resourceType":"common"}})
        }
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    Some(Ok(response))
}
