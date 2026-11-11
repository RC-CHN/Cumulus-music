//! Closed search and linked artist reads for the native primary-name marking proof.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const ALL: &str = "/api/search/pc/complex/page";
const SONGS: &str = "/api/search/song/list/page";
const LYRICS: &str = "/api/search/resource/lyric";
const ARTIST: &str = "/api/artist/v3/detail";
const ARTIST_DYNAMIC: &str = "/api/artist/detail/dynamic";
const RAW: &str = "原词";
const CORRECTED: &str = "Ａ＋Ｂ 樂隊";
const ARTIST_NAME: &str = "樂隊 乐队 + A+B 原词";
const EXCERPT: &str = "Ａ＋Ｂ 乐队 A+B 原词\n第二行 <img>";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub(super) fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

fn song() -> Value {
    json!({"id":31501,"name":"Ａ＋Ｂ AB A+B 夜场 原词 错误词","dt":186000,
        "ar":[{"id":31502,"name":ARTIST_NAME},{"name":"无链接 樂隊 乐队 + A+B 原词"}],
        "al":{"name":"<img> 乐队 / A+B 原词"},
        "privilege":{"id":31501,"fee":0,"st":0,"pl":320000,"dl":320000}})
}

pub(super) fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !matches!(
        path,
        ALL | SONGS | LYRICS | ARTIST | ARTIST_DYNAMIC | "/api/album/v3/detail"
    ) {
        return None;
    }
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    let correct = args["needCorrect"].as_bool();
    let channel = match correct {
        Some(true) => "typing",
        Some(false) => "suggest",
        None => "",
    };
    let payload = match path {
        ALL if correct.is_some()
            && *args
                == json!({"keyword":RAW,"scene":"normal","needCorrect":correct,
                "channel":channel,"cursor":"{\"page\":0,\"ignoreBlocks\":[],\"traceId\":\"\"}","bizQueryInfo":""}) =>
        {
            json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[
                {"type":"song","baseInfo":song()}
            ]}]}})
        }
        SONGS
            if correct.is_some()
                && *args
                    == json!({"keyword":RAW,"scene":"normal","limit":10,"offset":0,
                "needCorrect":correct,"channel":channel}) =>
        {
            // A server may repeat a correction after the user opts out. Native
            // must not mark using it in that visit.
            json!({"code":200,"data":{"resources":[{"type":"song",
                "baseInfo":{"simpleSongData":song()},"extInfo":{"lyrics":{"txt":"前原词后",
                    "range":[{"first":1,"second":3}]}}}],"totalCount":1,"hasMore":false,
                "queryRewriteInfo":{"rewriteQuery":CORRECTED}}})
        }
        LYRICS
            if matches!(args["keyword"].as_str(), Some(RAW | CORRECTED))
                && *args
                    == json!({"keyword":args["keyword"],"scene":"normal","limit":10,"offset":0}) =>
        {
            json!({"code":200,"data":{"resources":[{"song":song(),"lyrics":EXCERPT}],
                "totalCount":1,"hasMore":false,"queryRewriteInfo":{"rewriteQuery":"错误词"}}})
        }
        ARTIST if *args == json!({"id":"31502","top":50,"e_r":false}) => {
            json!({"code":200,"artist":{"id":31502,"name":ARTIST_NAME},"hotSongs":[]})
        }
        ARTIST_DYNAMIC if *args == json!({"id":"31502"}) => {
            json!({"code":200,"followed":false})
        }
        _ => return Some(Err(Error::Response("unexpected primary mark proof read"))),
    };
    Some(Ok(payload))
}
