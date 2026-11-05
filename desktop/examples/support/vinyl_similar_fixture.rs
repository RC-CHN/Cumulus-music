use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const EXPERIMENT: &str = "PH-PC-playPage-recommendMore-new";
const SONGS: &str = "/api/v1/discovery/simiSong";
const PLAYLISTS: &str = "/api/discovery/simiPlaylist";
const WIKI: &str = "/api/music/wiki/home/song/desktop/get";
const HELD_SEED: &str = "61";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static FAILED_FIRST: AtomicBool = AtomicBool::new(false);
static HOLDING: AtomicBool = AtomicBool::new(false);
static RELEASED: AtomicBool = AtomicBool::new(false);
static HELD_RETURNED: AtomicBool = AtomicBool::new(false);
static RELEASE: Notify = Notify::const_new();
static WIKI_FAILED_FIRST: AtomicBool = AtomicBool::new(false);
static WIKI_HOLDING: AtomicBool = AtomicBool::new(false);
static WIKI_RELEASED: AtomicBool = AtomicBool::new(false);
static WIKI_HELD_RETURNED: AtomicBool = AtomicBool::new(false);
static WIKI_RELEASE: Notify = Notify::const_new();

fn cohort() -> Option<String> {
    std::env::var("CUMULUS_VINYL_SIMILAR_PROOF")
        .ok()
        .filter(|group| matches!(group.as_str(), "t1" | "c"))
}

pub fn enabled() -> bool {
    cohort().is_some()
}

pub fn wiki_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_VINYL_WIKI_PROOF").is_some()
}

fn recommendations_enabled(expected: &str) -> bool {
    revision().as_deref() == Some(expected) && (cohort().as_deref() == Some("t1") || wiki_enabled())
}

pub fn revision() -> Option<String> {
    cohort().map(|group| format!("synthetic-vinyl-similar-{group}"))
}

pub fn groups(expected: &str) -> Result<PcExperimentGroups> {
    if revision().as_deref() != Some(expected) {
        return Err(Error::CredentialChanged);
    }
    record(json!({"lane":"assignment","name":EXPERIMENT,"group":cohort()}));
    PcExperimentGroups::refreshed(&json!({
        "data":[{"expName":EXPERIMENT,"expGroupName":cohort()}],
    }))
}

fn record(value: Value) {
    CALLS.lock().unwrap().push(value);
}

pub fn report() -> Value {
    json!({
        "calls":*CALLS.lock().unwrap(),
        "holding":HOLDING.load(Ordering::Acquire),
        "released":RELEASED.load(Ordering::Acquire),
        "heldReturned":HELD_RETURNED.load(Ordering::Acquire),
        "wikiHolding":WIKI_HOLDING.load(Ordering::Acquire),
        "wikiReleased":WIKI_RELEASED.load(Ordering::Acquire),
        "wikiHeldReturned":WIKI_HELD_RETURNED.load(Ordering::Acquire),
    })
}

pub fn control(command: &str) -> Result<Value> {
    match command {
        "status" => (),
        "release" if HOLDING.load(Ordering::Acquire) => {
            RELEASED.store(true, Ordering::Release);
            RELEASE.notify_one();
        }
        "release-wiki" if WIKI_HOLDING.load(Ordering::Acquire) => {
            WIKI_RELEASED.store(true, Ordering::Release);
            WIKI_RELEASE.notify_one();
        }
        _ => return Err(Error::InvalidInput("unknown sealed vinyl proof action")),
    }
    Ok(json!({
        "holding":HOLDING.load(Ordering::Acquire),
        "heldReturned":HELD_RETURNED.load(Ordering::Acquire),
        "wikiHolding":WIKI_HOLDING.load(Ordering::Acquire),
        "wikiHeldReturned":WIKI_HELD_RETURNED.load(Ordering::Acquire),
    }))
}

pub fn ordinary_request(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if matches!(path, SONGS | PLAYLISTS | WIKI) {
        // No raw signing material goes in screenshot evidence, even if a
        // regression accidentally tries to use the generic request lane.
        record(json!({"lane":"unbound","path":path}));
        return Some(Err(Error::InvalidInput("unbound vinyl proof request")));
    }
    if path == "/api/song/enhance/player/url/v1" {
        let ids = args["ids"]
            .as_str()
            .and_then(|ids| serde_json::from_str::<Vec<Value>>(ids).ok())
            .unwrap_or_default();
        record(json!({"lane":"audio","path":path,"ids":ids}));
    }
    if path == "/api/v6/playlist/detail" {
        record(json!({"lane":"detail","path":path,"id":args["id"]}));
    }
    None
}

pub async fn songs(path: &str, args: Value, expected: &str) -> Result<Value> {
    record(json!({"lane":"songs","path":path,"args":args}));
    if !recommendations_enabled(expected) {
        return Err(Error::CredentialChanged);
    }
    let seed = args["songid"]
        .as_str()
        .filter(|seed| args == json!({"songid":seed}) && path == SONGS)
        .ok_or(Error::InvalidInput("unexpected vinyl song query"))?;
    let control = cohort().as_deref() == Some("c");
    if !control && seed == "1" && !FAILED_FIRST.swap(true, Ordering::AcqRel) {
        return Ok(json!({"code":503}));
    }
    let ids: &[u32] = match seed {
        "1" if control => &[51, 52, 53, 54, 55],
        "1" => &[51, 52, 53],
        "51" => &[54, 55, 56],
        "55" => &[57, 58, 59],
        "58" => &[60, 61, 62],
        HELD_SEED => {
            HOLDING.store(true, Ordering::Release);
            tokio::time::timeout(Duration::from_secs(22), async {
                while !RELEASED.load(Ordering::Acquire) {
                    RELEASE.notified().await;
                }
            })
            .await
            .map_err(|_| Error::Transport("sealed old vinyl read was not released".into()))?;
            HELD_RETURNED.store(true, Ordering::Release);
            &[81, 82, 83]
        }
        "59" => &[84, 85, 86],
        _ => return Err(Error::InvalidInput("unexpected vinyl playback seed")),
    };
    Ok(json!({"code":200,"songs":ids.iter().map(|id| json!({
        "id":id,"name":format!("相似合成歌曲 {id}"),"ar":[{"id":14,"name":"合成歌手"}],
        "al":{"id":15,"name":"合成专辑"},"dt":235000,"fee":0,
        "privilege":{"id":id,"pl":320000,"st":0},
    })).collect::<Vec<_>>()}))
}

pub fn playlists(path: &str, args: Value, expected: &str) -> Result<Value> {
    if std::env::var_os("CUMULUS_VINYL_SIMILAR_RELATED_PROOF").is_none() {
        return Err(Error::State("related-playlist proof is not enabled"));
    }
    record(json!({"lane":"playlists","path":path,"args":args}));
    if !recommendations_enabled(expected) {
        return Err(Error::CredentialChanged);
    }
    let seed = args["songid"]
        .as_str()
        .filter(|seed| {
            path == PLAYLISTS
                && args == json!({"songid":seed})
                && matches!(*seed, "1" | "51" | "55" | "58" | HELD_SEED | "59")
        })
        .ok_or(Error::InvalidInput("unexpected vinyl playlist query"))?;
    Ok(json!({"code":200,"playlists":(101..=108).map(|id| json!({
        "id":id,"name":format!("相似合成歌单 {id} · {seed}"),"playCount":id * 10000,
    })).collect::<Vec<_>>()}))
}

pub async fn wiki(path: &str, args: Value, expected: &str) -> Result<Value> {
    if !wiki_enabled() || revision().as_deref() != Some(expected) {
        return Err(Error::CredentialChanged);
    }
    let seed = args["songId"]
        .as_str()
        .ok_or(Error::InvalidInput("missing sealed Wiki seed"))?;
    // Frozen vectors from the recovered canonical signature, not a call to
    // the implementation under proof.
    let signature = match seed {
        "1" => "dad333d71f110afd673dba166f25bdf9",
        "51" => "bb17f8ab9ca4adfabb333dc576e3a034",
        "59" => "c31d5175a8eb8d1c81b1ce2c43d73e1d",
        "61" => "9cfb6631cb676d03ae73a5cf387341fe",
        _ => return Err(Error::InvalidInput("unexpected sealed Wiki seed")),
    };
    let exact = path == WIKI && args == json!({"songId":seed,"_scver":1,"_sign":signature});
    record(json!({"lane":"wiki","path":path,"songId":seed,"signatureVerified":exact}));
    if !exact {
        return Err(Error::InvalidInput("unexpected sealed Wiki request"));
    }
    if seed == "1" && !WIKI_FAILED_FIRST.swap(true, Ordering::AcqRel) {
        return Ok(json!({"code":503}));
    }
    if seed == "59" {
        WIKI_HOLDING.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(22), async {
            while !WIKI_RELEASED.load(Ordering::Acquire) {
                WIKI_RELEASE.notified().await;
            }
        })
        .await
        .map_err(|_| Error::Transport("sealed old Wiki read was not released".into()))?;
        WIKI_HELD_RETURNED.store(true, Ordering::Release);
    }
    Ok(json!({"code":200,"data":{"wikiSubBlockBaseInfoVo":{
        "wikiUrl":"https://music.163.com/NOT_PUBLIC_WIKI",
        "wikiSubElementVos":[
            {"title":"演唱","content":format!("合成百科 {seed}")},
            {"title":"曲风","pattern":1,"wikiSubMetaVos":[{"text":"轻音乐"},{"text":"钢琴"}]},
            {"title":"原样文本","content":"<b>合成内容</b>"},
            {"title":"","content":"NOT_PUBLIC_WIKI"},
        ],
    }}}))
}
