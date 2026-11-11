//! Closed Search Song comment proof. No request reaches an outside service.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::sync::Notify;

const SONGS: &str = "/api/search/song/list/page";
const COUNT: &str = "/api/resource/commentInfo/list";
const COMMENTS: &str = "/api/v1/resource/comments/R_SO_4_3";
const HOT_COMMENTS: &str = "/api/v1/resource/hotcomments/R_SO_4_3";
const WORD: &str = "合成评论菜单";
const REOPEN_WORD: &str = "合成评论菜单再查";

struct State {
    results: Vec<Value>,
    counts: Vec<Value>,
    player_badge_reads: Vec<Value>,
    threads: Vec<Value>,
    writes: Vec<Value>,
    held_started: bool,
    held_released: bool,
    held_returned: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    results: Vec::new(),
    counts: Vec::new(),
    player_badge_reads: Vec::new(),
    threads: Vec::new(),
    writes: Vec::new(),
    held_started: false,
    held_released: false,
    held_returned: false,
});
static RELEASE: Notify = Notify::const_new();

pub fn enabled() -> bool {
    std::env::var("CUMULUS_SEARCH_SONG_COMMENT_PROOF").as_deref() == Ok("1")
}

pub fn credential_revision() -> String {
    if STATE.lock().unwrap().held_released {
        "synthetic-search-song-comment-reopened"
    } else {
        "synthetic-search-song-comment-initial"
    }
    .into()
}

pub fn control(action: &str) {
    match action {
        "observe" => (),
        "release" => {
            let mut state = STATE.lock().unwrap();
            assert!(state.held_started && !state.held_released);
            // Also invalidate the credential revision to exercise native late-response
            // fencing independently of the renderer abandoning the previous visit.
            state.held_released = true;
            RELEASE.notify_one();
        }
        _ => panic!("unknown synthetic search Song comment fixture action"),
    }
}

pub fn report() -> Value {
    let state = STATE.lock().unwrap();
    json!({
        "results":state.results,"counts":state.counts,"playerBadgeReads":state.player_badge_reads,
        "threads":state.threads,"writes":state.writes,
        "heldStarted":state.held_started,"heldReleased":state.held_released,"heldReturned":state.held_returned,
    })
}

pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if path == SONGS {
        STATE
            .lock()
            .unwrap()
            .results
            .push(json!({"path":path,"args":args,"cached":cached}));
        if !matches!(args["keyword"].as_str(), Some(WORD | REOPEN_WORD)) || args["offset"] != 0 {
            return Some(Err(Error::Response(
                "unexpected synthetic Search Song page",
            )));
        }
        let rows = (3..=8)
            .map(|id| {
                let mut song = super::song(id);
                if id == 7 {
                    song["t"] = json!(1);
                }
                if id == 3 {
                    // A charged or down public Song still has public comments.
                    song["fee"] = json!(1);
                    song["privilege"] = json!({"id":id,"st":-200,"pl":0});
                }
                json!({"type":"song","baseInfo":{"simpleSongData":song}})
            })
            .collect::<Vec<_>>();
        return Some(Ok(
            json!({"code":200,"data":{"resources":rows,"hasMore":false,"totalCount":6}}),
        ));
    }
    if path == COUNT {
        return Some(count(args, cached).await);
    }
    if path.starts_with("/api/v1/comment/")
        || path.starts_with("/api/comment/")
        || matches!(
            path,
            "/api/resource/comments/add"
                | "/api/resource/comments/delete"
                | "/api/v1/resource/comments/reply"
        )
    {
        STATE
            .lock()
            .unwrap()
            .writes
            .push(json!({"path":path,"args":args}));
        return Some(Err(Error::State(
            "synthetic Search Song comments never authorize writes",
        )));
    }
    if path.starts_with("/api/v1/resource/comments/")
        || path.starts_with("/api/v1/resource/hotcomments/")
    {
        STATE
            .lock()
            .unwrap()
            .threads
            .push(json!({"path":path,"args":args,"cached":cached}));
        if !matches!(path, COMMENTS | HOT_COMMENTS) || cached || args["offset"] != 0 {
            return Some(Err(Error::Response(
                "wrong synthetic Search Song comment thread",
            )));
        }
        let mut page = json!({"code":200,"total":36,"more":false,"hasMore":false,
            "comments":[],"hotComments":[],"topComments":[]});
        page[if path == COMMENTS {
            "comments"
        } else {
            "hotComments"
        }] = json!([{
            "commentId":18308,"content":"合成搜索点中单曲评论","time":1700000000000_u64,
            "liked":false,"likedCount":0,"user":{"userId":3,"nickname":"合成听友"},"beReplied":[],
        }]);
        return Some(Ok(page));
    }
    None
}

async fn count(args: &Value, cached: bool) -> Result<Value> {
    let id = args["resourceIds"]
        .as_str()
        .and_then(|raw| serde_json::from_str::<[String; 1]>(raw).ok())
        .filter(|ids| {
            !cached
                && *args
                    == json!({"resourceType":4,"resourceIds":serde_json::to_string(ids).unwrap()})
        });
    if id.as_ref().is_some_and(|ids| ids[0] == "91") {
        // The baseline paused decoder has its own real minibar comment read.
        // A positive synthetic badge lets the renderer prove it has settled.
        let (thread, count) = ("R_SO_4_91", 17);
        STATE.lock().unwrap().player_badge_reads.push(json!({
            "path":COUNT,"args":args,"cached":cached,"thread":thread,"count":count,
        }));
        return Ok(json!({"code":200,"data":[{"threadId":thread,"commentCount":count}]}));
    }
    let (thread, value, outcome) = {
        let mut state = STATE.lock().unwrap();
        let (thread, value, outcome) = match id.as_ref().map(|ids| ids[0].as_str()) {
            Some("3") => (Some("R_SO_4_3"), Some(36), "count"),
            Some("4") => (Some("R_SO_4_4"), Some(0), "zero"),
            Some("5") => (Some("R_SO_4_9"), Some(99), "wrong_thread"),
            Some("6") => (None, None, "transport"),
            Some("8") if !state.held_started => {
                state.held_started = true;
                (Some("R_SO_4_8"), Some(88), "held")
            }
            Some("8") if state.held_released => (Some("R_SO_4_8"), Some(51), "reopened"),
            _ => (None, None, "unexpected"),
        };
        state
            .counts
            .push(json!({"path":COUNT,"args":args,"cached":cached,
            "thread":thread,"count":value,"outcome":outcome}));
        (thread, value, outcome)
    };
    match outcome {
        "unexpected" => return Err(Error::Response("unexpected synthetic Search Song count")),
        "transport" => {
            return Err(Error::Transport(
                "synthetic Search Song count offline".into(),
            ));
        }
        "held" => {
            RELEASE.notified().await;
            STATE.lock().unwrap().held_returned = true;
        }
        _ => (),
    }
    Ok(json!({"code":200,"data":[{"threadId":thread,"commentCount":value}]}))
}
