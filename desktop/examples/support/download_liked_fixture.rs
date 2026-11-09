//! Synthetic account authority for the downloaded row's system liked target.
use cumulus_core::{Error, Result, protocol::api::SystemLikedRead};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Default)]
struct State {
    reads: Vec<Value>,
    writes: Vec<Value>,
    generic_likes: u64,
    added: bool,
    member: bool,
}

static STATE: std::sync::LazyLock<Mutex<State>> =
    std::sync::LazyLock::new(|| Mutex::new(State::default()));

pub fn enabled() -> bool {
    super::download_page_fixture::collection_enabled()
        && std::env::var_os("CUMULUS_DOWNLOAD_COLLECTION_LIKED_PROOF").is_some()
}

pub const REVISION: &str = "sealed-downloaded-system-liked";

fn row(state: &State) -> Value {
    let mut row = super::playlist(83);
    row["name"] = json!("名称改变的系统歌单");
    row["userId"] = json!(2);
    row["creator"]["userId"] = json!(2);
    row["specialType"] = json!(5);
    row["trackIds"] = if state.added {
        json!([{"id":1,"v":0},{"id":3,"v":0},{"id":7,"v":0},{"id":10,"v":0}])
    } else {
        json!([{"id":3,"v":0},{"id":7,"v":0},{"id":10,"v":0}])
    };
    row
}

pub fn read(read: SystemLikedRead) -> Result<Value> {
    assert!(enabled());
    let (path, args) = read.into_request();
    let mut state = STATE.lock().unwrap();
    state.reads.push(json!({"path":path,"args":args}));
    Ok(match path {
        "/api/user/playlist" => {
            assert_eq!(args, json!({"uid":"2","offset":0,"limit":1000}));
            let mut decoy = super::playlist(2);
            decoy["name"] = json!("我喜欢的音乐");
            json!({"code":200,"more":false,"playlist":[decoy,row(&state)]})
        }
        "/api/v6/playlist/detail" => {
            assert_eq!(args, json!({"id":"83","n":100000,"s":0}));
            json!({"code":200,"playlist":row(&state)})
        }
        "/api/song/like/get" => {
            assert_eq!(args, json!({"uid":"2"}));
            json!({"code":200,"ids":if state.member {json!([3,7,10,1])} else {json!([3,7,10])}})
        }
        _ => panic!("unexpected download liked read"),
    })
}

pub fn observe_generic_likes() {
    if enabled() {
        STATE.lock().unwrap().generic_likes += 1;
    }
}

pub fn write(args: Value) -> Result<Value> {
    assert!(enabled());
    assert_eq!(args, json!({"pid":"83","trackIds":"[\"1\"]","op":"add"}));
    let mut state = STATE.lock().unwrap();
    assert!(state.writes.is_empty());
    state.writes.push(args);
    state.added = true;
    Err(Error::Transport(
        "sealed download collection response lost".into(),
    ))
}

pub fn settle() {
    let mut state = STATE.lock().unwrap();
    assert!(state.added && !state.member);
    state.member = true;
}

pub fn report(store: &cumulus_core::storage::StateStore) -> Value {
    let state = STATE.lock().unwrap();
    json!({"reads":state.reads,"writes":state.writes,"genericLikes":state.generic_likes,
        "cachedLikes":store.get("2","resources","liked").unwrap()})
}
