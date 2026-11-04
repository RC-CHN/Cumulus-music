use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static GUEST_REVOKED: AtomicBool = AtomicBool::new(false);
static WEEK_84_REMOVED: AtomicBool = AtomicBool::new(false);
static WEEK_86_REMOVED: AtomicBool = AtomicBool::new(false);
static ALL_85_ABSENT: AtomicBool = AtomicBool::new(false);
static DELETE_RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();
const REMOVE: &str = "/api/play/record/empty";

pub fn removal_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_PROFILE_RANK_REMOVAL_PROOF").is_some()
}

pub fn release_remove() {
    DELETE_RELEASE.notify_one();
}

pub fn remove_all_row_externally() {
    ALL_85_ABSENT.store(true, Ordering::Release);
}

pub async fn hold_response(path: &str, args: &Value) {
    if removal_enabled() && path == REMOVE && args == &json!({"type":"1","songId":"84"}) {
        DELETE_RELEASE.notified().await;
    }
}

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_PROFILE_PLAY_RECORD_PROOF").is_some()
}

pub fn revoke_guest() {
    GUEST_REVOKED.store(true, Ordering::Release);
}

pub fn report() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

pub fn decorate_playlist(id: u64, playlist: &mut Value) {
    if enabled() && matches!(id, 21 | 22) {
        let uid = if id == 21 { 4 } else { 5 };
        playlist["userId"] = json!(uid);
        playlist["creator"] = json!({"userId":uid,"nickname":format!("合成听友 {uid}")});
        playlist["privacy"] = json!(0);
    }
}

fn rank_row(id: u64, title: &str, count: u64) -> Value {
    json!({"playCount":count,"score":100,"song":{
        "id":id,"name":title,"ar":[{"id":12,"name":"合成排行歌手"}],
        "al":{"id":20,"name":"合成排行专辑"},"dt":30000
    }})
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let uid = match path {
        "/api/w/v1/user/detail/2" => Some(2),
        "/api/w/v1/user/detail/3" => Some(3),
        "/api/w/v1/user/detail/4" => Some(4),
        "/api/w/v1/user/detail/5" => Some(5),
        "/api/v1/play/record" | REMOVE => None,
        "/api/user/playlist" if matches!(args["uid"].as_str(), Some("2" | "4" | "5")) => {
            return Some(Ok(json!({"code":200,"more":false,"playlist":[]})));
        }
        _ => return None,
    };
    let mut calls = CALLS.lock().unwrap();
    calls.push(json!({"path":path,"args":args,"cached":cached}));
    if cached {
        return Some(Err(Error::Response("cached synthetic rank request")));
    }
    if let Some(uid) = uid {
        if args != &json!({"all":true,"userId":uid.to_string()}) {
            return Some(Err(Error::Response("wrong synthetic rank profile request")));
        }
        let name = if uid == 2 {
            "合成自己".into()
        } else {
            format!("合成听友 {uid}")
        };
        let mut detail = json!({"code":200,"listenSongs":if uid == 2 {7890} else {987654},
            "profile":{"userId":uid,"nickname":name,"follows":3,"followeds":1}});
        match uid {
            3 => detail["peopleCanSeeMyPlayRecord"] = json!(!GUEST_REVOKED.load(Ordering::Acquire)),
            5 => detail["peopleCanSeeMyPlayRecord"] = json!(false),
            // Self still sees their own rank; the missing guest never sees it.
            2 | 4 => {}
            _ => unreachable!(),
        }
        return Some(Ok(detail));
    }
    if path == REMOVE {
        if !removal_enabled() {
            return Some(Err(Error::Response("unexpected synthetic rank removal")));
        }
        return Some(if args == &json!({"type":"1","songId":"84"}) {
            WEEK_84_REMOVED.store(true, Ordering::Release);
            Ok(json!({"code":200}))
        } else if args == &json!({"type":"1","songId":"86"}) {
            WEEK_86_REMOVED.store(true, Ordering::Release);
            Ok(json!({"code":200}))
        } else if args == &json!({"type":"0","songId":"85"}) {
            Err(Error::Transport("synthetic rank removal reply lost".into()))
        } else {
            Err(Error::Response("unexpected synthetic rank removal body"))
        });
    }
    let kind = args["type"].as_u64().unwrap_or(u64::MAX);
    let uid = args["uid"].as_str().unwrap_or_default();
    if args != &json!({"offset":0,"total":true,"limit":120,"uid":uid,"type":kind})
        || !matches!(kind, 0 | 1)
        || !matches!(uid, "2" | "3")
        || uid == "3" && GUEST_REVOKED.load(Ordering::Acquire)
    {
        return Some(Err(Error::Response(
            "wrong or unapproved synthetic rank request",
        )));
    }
    if !removal_enabled()
        && uid == "2"
        && kind == 0
        && calls
            .iter()
            .filter(|c| c["path"] == path && c["args"] == *args)
            .count()
            == 1
    {
        return Some(Err(Error::Transport("synthetic rank tab failed".into())));
    }
    if removal_enabled() && uid == "2" {
        let rows = if kind == 1 {
            [
                (!WEEK_84_REMOVED.load(Ordering::Acquire)).then(|| rank_row(84, "本人周榜", 4321)),
                (!WEEK_86_REMOVED.load(Ordering::Acquire))
                    .then(|| rank_row(86, "本人周榜第二", 4322)),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
        } else {
            [
                Some(rank_row(84, "同期总榜同一首", 8764)),
                (!ALL_85_ABSENT.load(Ordering::Acquire)).then(|| rank_row(85, "本人总榜", 8765)),
            ]
            .into_iter()
            .flatten()
            .collect()
        };
        let field = if kind == 1 { "weekData" } else { "allData" };
        return Some(Ok(json!({"code":200, (field): rows})));
    }
    Some(Ok(match (uid, kind) {
        ("3", 1) => {
            json!({"code":200,"weekData":[rank_row(81,"访客周榜第一",654321),rank_row(82,"访客周榜第二",654322)]})
        }
        ("3", 0) => json!({"code":200,"allData":[rank_row(83,"访客总榜第一",654323)]}),
        ("2", 1) => json!({"code":200,"weekData":[rank_row(84,"本人周榜",4321)]}),
        ("2", 0) => json!({"code":200,"allData":[rank_row(85,"本人总榜",8765)]}),
        _ => unreachable!(),
    }))
}
