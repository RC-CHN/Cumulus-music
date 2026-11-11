//! Sealed native Lyrics search/comment transport; the player badge has a separate ledger.
use cumulus_core::{Error, Result, protocol::api::SearchSongCommentSource};
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::sync::Notify;

const LYRICS: &str = "/api/search/resource/lyric";
pub const COUNT: &str = "/api/resource/commentInfo/list";
const PAGE: &str = "/api/v1/resource/comments/R_SO_4_3";
const HOT: &str = "/api/v1/resource/hotcomments/R_SO_4_3";
const WORD: &str = "合成歌词评论";
const REOPEN_WORD: &str = "合成歌词评论再查";

#[derive(Default)]
struct State {
    results: Vec<Value>,
    scoped: Vec<Value>,
    badge: Vec<Value>,
    threads: Vec<Value>,
    unexpected: Vec<Value>,
    held: bool,
    released: bool,
    returned: bool,
}
static STATE: std::sync::LazyLock<Mutex<State>> =
    std::sync::LazyLock::new(|| Mutex::new(State::default()));
static RELEASE: Notify = Notify::const_new();

pub fn enabled() -> bool {
    std::env::var("CUMULUS_SEARCH_LYRIC_COMMENT_PROOF").as_deref() == Ok("1")
}

pub fn revision() -> String {
    if STATE.lock().unwrap().released {
        "synthetic-lyric-comments-reopened"
    } else {
        "synthetic-lyric-comments-initial"
    }
    .into()
}

pub fn control(action: &str) -> Value {
    let mut state = STATE.lock().unwrap();
    if action == "release" {
        assert!(state.held && !state.released);
        state.released = true;
        RELEASE.notify_one();
    } else {
        assert_eq!(action, "observe");
    }
    report(&state)
}

fn report(state: &State) -> Value {
    json!({
        "results":state.results,"counts":state.scoped,"playerBadgeReads":state.badge,
        "threads":state.threads,"unexpected":state.unexpected,
        "held":state.held,"released":state.released,"returned":state.returned,
    })
}

pub fn receipt() -> Value {
    report(&STATE.lock().unwrap())
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let call = || json!({"path":path,"args":args,"cached":cached});
    let mut state = STATE.lock().unwrap();
    if path == LYRICS {
        state.results.push(call());
        let keyword = args["keyword"].as_str();
        if !matches!(keyword, Some(WORD | REOPEN_WORD))
            || cached
            || *args != json!({"keyword":keyword,"scene":"normal","limit":10,"offset":0})
        {
            return Some(Err(Error::Response(
                "unexpected synthetic Lyrics results query",
            )));
        }
        let resources = [3, 3, 4, 5, 7, 8]
            .into_iter()
            .enumerate()
            .map(|(index, id)| {
                let mut song = super::song(id);
                if id == 7 {
                    song["t"] = json!(1);
                }
                json!({"song":song,"lyrics":format!("合成命中第{}行", index + 1)})
            })
            .collect::<Vec<_>>();
        return Some(Ok(
            json!({"code":200,"data":{"totalCount":6,"resources":resources}}),
        ));
    }
    if path == COUNT && !cached && count_id(args) == Some("91") {
        state.badge.push(call());
        return Some(Ok(count_response("R_SO_4_91", 17)));
    }
    if path == COUNT
        || path.starts_with("/api/v1/comment/")
        || path.starts_with("/api/comment/")
        || matches!(
            path,
            "/api/resource/comments/add"
                | "/api/resource/comments/delete"
                | "/api/v1/resource/comments/reply"
        )
    {
        state.unexpected.push(call());
        return Some(Err(Error::State(
            "unexpected unscoped count or comment write",
        )));
    }
    if path.starts_with("/api/v1/resource/comments/")
        || path.starts_with("/api/v1/resource/hotcomments/")
    {
        state.threads.push(call());
        if !matches!(path, PAGE | HOT) || cached || args["offset"] != 0 {
            return Some(Err(Error::Response("wrong synthetic Lyric comment thread")));
        }
        let mut page = json!({"code":200,"total":36,"more":false,"hasMore":false,
            "comments":[],"hotComments":[],"topComments":[]});
        page[if path == PAGE {
            "comments"
        } else {
            "hotComments"
        }] = json!([{
            "commentId":19309,"content":"合成歌词点中歌曲评论","time":1700000000000_u64,
            "liked":false,"likedCount":0,"user":{"userId":3,"nickname":"合成听友"},"beReplied":[],
        }]);
        return Some(Ok(page));
    }
    None
}

fn count_id(args: &Value) -> Option<&str> {
    if args["resourceType"] != 4 || args.as_object()?.len() != 2 {
        return None;
    }
    match args["resourceIds"].as_str()? {
        "[\"3\"]" => Some("3"),
        "[\"4\"]" => Some("4"),
        "[\"5\"]" => Some("5"),
        "[\"8\"]" => Some("8"),
        "[\"91\"]" => Some("91"),
        _ => None,
    }
}

fn count_response(thread: &str, count: u64) -> Value {
    json!({"code":200,"data":[{"threadId":thread,"commentCount":count}]})
}

pub async fn scoped_count(source: SearchSongCommentSource, args: &Value) -> Result<Value> {
    let source = match source {
        SearchSongCommentSource::Lyrics => "lyrics",
        SearchSongCommentSource::Songs => "songs",
    };
    let (thread, count, kind) = {
        let mut state = STATE.lock().unwrap();
        let (thread, count, kind) = match (source, count_id(args), state.scoped.len()) {
            ("lyrics", Some("3"), 0) => (Some("R_SO_4_3"), Some(36), "first"),
            ("lyrics", Some("3"), 1) => (Some("R_SO_4_3"), Some(42), "duplicate"),
            ("lyrics", Some("4"), 2) => (Some("R_SO_4_4"), Some(0), "zero"),
            ("lyrics", Some("5"), 3) => (None, None, "transport"),
            ("lyrics", Some("8"), 4) => {
                state.held = true;
                (Some("R_SO_4_8"), Some(88), "held")
            }
            ("lyrics", Some("8"), 5) if state.released => (Some("R_SO_4_8"), Some(51), "reopened"),
            _ => (None, None, "unexpected"),
        };
        state.scoped.push(
            json!({"source":source,"path":COUNT,"args":args,"cached":false,
            "thread":thread,"count":count,"kind":kind}),
        );
        (thread, count, kind)
    };
    match kind {
        "transport" => return Err(Error::Transport("synthetic Lyrics count offline".into())),
        "unexpected" => return Err(Error::Response("wrong source or synthetic Lyrics count")),
        "held" => {
            RELEASE.notified().await;
            STATE.lock().unwrap().returned = true;
        }
        _ => (),
    }
    Ok(count_response(thread.unwrap(), count.unwrap()))
}
