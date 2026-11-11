//! Closed native playlist Song comment proof; never authorizes writes.
use cumulus_core::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::sync::Notify;

const COUNT: &str = "/api/resource/commentInfo/list";
const COMMENTS: &str = "/api/v1/resource/comments/R_SO_4_3";
const HOT_COMMENTS: &str = "/api/v1/resource/hotcomments/R_SO_4_3";
const PLAYLIST: &str = "/api/v6/playlist/detail";
const SONGS: &str = "/api/v3/song/detail";

struct State {
    counts: Vec<Value>,
    threads: Vec<Value>,
    catalog_reads: Vec<Value>,
    held_started: bool,
    held_released: bool,
    held_returned: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    counts: Vec::new(),
    threads: Vec::new(),
    catalog_reads: Vec::new(),
    held_started: false,
    held_released: false,
    held_returned: false,
});
static RELEASE: Notify = Notify::const_new();

pub fn enabled() -> bool {
    std::env::var("CUMULUS_PLAYLIST_SONG_COMMENT_PROOF").as_deref() == Ok("1")
}

pub fn decorate_song(id: u64, value: &mut Value) {
    if !matches!(id, 7 | 9) || !enabled() {
        return;
    }
    if id == 7 {
        // M/21's songType=1, distinct from an ordinary copyrighted Song.
        value["t"] = json!(1);
    } else {
        value["dt"] = json!(42000);
    }
}

pub fn decorate_playlist(id: u64, value: &mut Value) {
    if id == 2 && enabled() {
        value["trackIds"] = json!(
            (1..=105)
                .map(|id| json!({"id":id,"v":0}))
                .collect::<Vec<_>>()
        );
        value["trackCount"] = json!(105);
    }
}

pub fn control(action: &str) {
    match action {
        "observe" => (),
        "release" => {
            let mut state = STATE.lock().unwrap();
            assert!(state.held_started && !state.held_released);
            state.held_released = true;
            RELEASE.notify_one();
        }
        _ => panic!("unknown playlist Song comment fixture action"),
    }
}

pub fn report() -> Value {
    let state = STATE.lock().unwrap();
    json!({
        "counts":state.counts,"threads":state.threads,"catalogReads":state.catalog_reads,"heldStarted":state.held_started,
        "heldReleased":state.held_released,"heldReturned":state.held_returned,
    })
}

pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if path == COUNT {
        return Some(count(args, cached).await);
    }
    if path == PLAYLIST {
        STATE
            .lock()
            .unwrap()
            .catalog_reads
            .push(json!({"path":path,"id":identifier(&args["id"]).ok(),"cached":cached}));
    } else if path == SONGS {
        let ids = args["c"]
            .as_str()
            .and_then(|raw| serde_json::from_str::<Vec<Value>>(raw).ok())
            .and_then(|rows| {
                rows.iter()
                    .map(|row| identifier(&row["id"]))
                    .collect::<Result<Vec<_>>>()
                    .ok()
            });
        STATE
            .lock()
            .unwrap()
            .catalog_reads
            .push(json!({"path":path,"ids":ids,"cached":cached}));
    }
    if matches!(path, COMMENTS | HOT_COMMENTS) {
        STATE
            .lock()
            .unwrap()
            .threads
            .push(json!({"path":path,"args":args,"cached":cached}));
        if cached || args["offset"] != 0 {
            return Some(Err(Error::Response(
                "unexpected synthetic playlist Song comment page",
            )));
        }
        let mut page = json!({"code":200,"total":36,"more":false,
            "comments":[],"hotComments":[],"topComments":[]});
        page[if path == COMMENTS {
            "comments"
        } else {
            "hotComments"
        }] = json!([{
            "commentId":17306,"content":"合成歌单点中歌曲评论","time":1700000000000_u64,
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
                    == json!({
                        "resourceType":4,"resourceIds":serde_json::to_string(ids).unwrap(),
                    })
        });
    let (thread, value, outcome) = {
        let mut state = STATE.lock().unwrap();
        let (thread, value, outcome) = if let Some([id]) = id {
            match id.as_str() {
                "3" => (Some("R_SO_4_3"), Some(36), "count"),
                "4" => (Some("R_SO_4_4"), Some(0), "zero"),
                "5" => (Some("R_SO_4_9"), Some(99), "wrong_thread"),
                "6" => (None, None, "transport"),
                "8" if !state.held_started => {
                    state.held_started = true;
                    (Some("R_SO_4_8"), Some(88), "held")
                }
                "8" if state.held_released => (Some("R_SO_4_8"), Some(51), "reopened"),
                "9" => match state
                    .counts
                    .iter()
                    .filter(|call| call["args"]["resourceIds"] == "[\"9\"]")
                    .count()
                {
                    0 => (Some("R_SO_4_9"), Some(64), "sorted"),
                    1 => (Some("R_SO_4_9"), Some(64), "restored_order"),
                    _ => (None, None, "unexpected"),
                },
                "101" => match state
                    .counts
                    .iter()
                    .filter(|call| call["args"]["resourceIds"] == "[\"101\"]")
                    .count()
                {
                    0 => (Some("R_SO_4_101"), Some(70), "suffix"),
                    1 => (Some("R_SO_4_101"), Some(73), "retained_back"),
                    _ => (None, None, "unexpected"),
                },
                _ => (None, None, "unexpected"),
            }
        } else {
            (None, None, "unexpected")
        };
        state
            .counts
            .push(json!({"path":COUNT,"args":args,"cached":cached,
            "thread":thread,"count":value,"outcome":outcome}));
        (thread, value, outcome)
    };
    match outcome {
        "unexpected" => {
            return Err(Error::Response(
                "unexpected synthetic playlist Song count payload",
            ));
        }
        "transport" => {
            return Err(Error::Transport(
                "synthetic playlist Song count offline".into(),
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
