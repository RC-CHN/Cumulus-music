//! Sealed lyric group playback data and native request receipts.
use cumulus_core::{Error, Result, protocol::api::SearchSongCommentSource};
use serde_json::{Value, json};
use std::sync::{LazyLock, Mutex};

const LYRICS: &str = "/api/search/resource/lyric";
const WORD: &str = "合成歌词组选曲";
const REOPEN: &str = "合成歌词组选曲再查";
pub const CREDENTIAL_REVISION: &str = "synthetic-lyric-group-playback";

#[derive(Default)]
struct State {
    lyrics: Vec<Value>,
    media: Vec<Value>,
    row_counts: Vec<Value>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

pub fn enabled() -> bool {
    std::env::var("CUMULUS_SEARCH_LYRIC_GROUP_PROOF").as_deref() == Ok("1")
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() || path != LYRICS {
        return None;
    }
    STATE
        .lock()
        .unwrap()
        .lyrics
        .push(json!({"path":path,"args":args,"cached":cached}));
    let keyword = args["keyword"].as_str();
    let offset = args["offset"].as_u64();
    if cached
        || !matches!(keyword, Some(WORD | REOPEN))
        || *args != json!({"keyword":keyword,"scene":"normal","limit":10,"offset":offset})
    {
        return Some(Err(Error::Response("unexpected lyric group query")));
    }
    let ids: &[u64] = match offset {
        Some(0) => &[101, 102, 103, 101, 104, 105, 106, 107, 108, 109],
        Some(10) => &[110, 111, 112],
        _ => return Some(Err(Error::Response("unexpected lyric group offset"))),
    };
    let resources = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let mut song = super::song(id);
            let (st, pl) = if id == 102 { (-200, 0) } else { (0, 320000) };
            song["privilege"] = json!({"id":id,"st":st,"pl":pl});
            json!({"song":song,"lyrics":format!("合成歌词组命中第{}行", index as u64+offset.unwrap()+1)})
        })
        .collect::<Vec<_>>();
    Some(Ok(
        json!({"code":200,"data":{"totalCount":13,"resources":resources}}),
    ))
}

pub fn record_media(ids: &[Value], args: &Value) -> Result<()> {
    if !enabled() {
        return Ok(());
    }
    let [id] = ids else {
        return Err(Error::Response("expected one exact lyric media track"));
    };
    let id = id.as_u64().or_else(|| id.as_str()?.parse().ok());
    STATE
        .lock()
        .unwrap()
        .media
        .push(json!({"id":id,"trialMode":args["trialMode"],"level":args["level"]}));
    match id {
        Some(91 | 92) => Ok(()),
        Some(101 | 103..=112) if args["trialMode"] == 42 => Ok(()),
        _ => Err(Error::Response(
            "wrong scoped lyric media track or trial mode",
        )),
    }
}

pub fn record_menu_count(source: SearchSongCommentSource, args: &Value) -> Result<Value> {
    let source_name = match source {
        SearchSongCommentSource::Lyrics => "Lyrics",
        SearchSongCommentSource::Songs => "Songs",
    };
    STATE
        .lock()
        .unwrap()
        .row_counts
        .push(json!({"source":source_name,"args":args}));
    if matches!(source, SearchSongCommentSource::Lyrics)
        && *args == json!({"resourceType":4,"resourceIds":"[\"104\"]"})
    {
        return Ok(json!({"code":200,"data":[{"threadId":"R_SO_4_104","commentCount":3}]}));
    }
    Err(Error::Response("unexpected lyric menu count"))
}

pub fn receipt() -> Value {
    let state = STATE.lock().unwrap();
    json!({"lyrics":state.lyrics,"media":state.media,"rowCounts":state.row_counts})
}
