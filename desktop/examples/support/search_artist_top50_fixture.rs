//! Sealed two-cohort search artist playback oracle.
use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::sync::Mutex;

const HOT: &str = "/api/artist/v3/detail";
const SONGS: &str = "/api/v2/artist/songs";
const PRIVILEGE: &str = "/api/song/enhance/privilege";
const PLAYER_URL: &str = "/api/song/enhance/player/url/v1";
const EXPERIMENT: &str = "PH-PC-Singer-Top50";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

fn group() -> Option<String> {
    std::env::var("CUMULUS_SEARCH_ARTIST_TOP50_PROOF")
        .ok()
        .filter(|group| matches!(group.as_str(), "t0" | "t1"))
}

pub fn enabled() -> bool {
    super::search_fixture::enabled() && group().is_some()
}

pub fn revision() -> Option<String> {
    enabled().then(|| format!("sealed-search-artist-{}", group().unwrap()))
}

pub fn groups(revision: &str) -> Result<PcExperimentGroups> {
    if self::revision().as_deref() != Some(revision) {
        return Err(Error::CredentialChanged);
    }
    let group = group().unwrap();
    CALLS.lock().unwrap().push(json!({"assignment":group}));
    PcExperimentGroups::refreshed(&json!({"data":[{"expName":EXPERIMENT,"expGroupName":group}]}))
}

pub fn report() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

fn ids(artist: &str) -> Option<[u64; 3]> {
    match artist {
        "101" => Some([92, 91, 93]),
        "102" => Some([96, 94, 95]),
        "103" => Some([98, 97, 99]),
        _ => None,
    }
}

fn packed(args: &Value, key: &str) -> Vec<String> {
    let values: Vec<Value> = serde_json::from_str(args[key].as_str().unwrap()).unwrap();
    values
        .iter()
        .map(|value| cumulus_core::protocol::api::identifier(value).unwrap())
        .collect()
}

fn songs(artist: &str, ids: [u64; 3], control: bool) -> Vec<Value> {
    ids.into_iter()
        .map(|id| {
            let mut row = json!({"id":id,"name":format!("合成歌手热歌 {id}"),
                "ar":[{"id":artist,"name":"合成歌手"}],"al":{"name":"合成热歌专辑"},
                "dt":30000,"fee":0});
            if control {
                row["privilege"] = privilege(id);
            }
            row
        })
        .collect()
}

fn privilege(id: u64) -> Value {
    json!({"id":id,"pl":320000,"maxbr":999000,"flag":0,"st":0})
}

fn record(path: &str, args: &Value, cache: bool) {
    CALLS
        .lock()
        .unwrap()
        .push(json!({"path":path,"args":args,"cache":cache}));
}

pub fn response(path: &str, args: &Value, cache: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let treatment = group().as_deref() == Some("t1");
    match path {
        HOT | SONGS => {
            let artist = args["id"].as_str()?;
            let ids = ids(artist)?;
            record(path, args, cache);
            let response = if path == HOT && treatment {
                assert!(cache);
                assert_eq!(*args, json!({"id":artist,"top":50,"e_r":false}));
                Ok(json!({"code":200,"artist":{"id":artist,"name":"合成歌手"},
                    "hotSongs":songs(artist,ids,false)}))
            } else if path == SONGS && !treatment {
                assert!(!cache);
                assert_eq!(*args, json!({"id":artist,"limit":50,"offset":0}));
                if artist == "103" {
                    Err(Error::Transport("合成歌手热歌读取失败".into()))
                } else {
                    Ok(json!({"code":200,"songs":songs(artist,ids,true),"more":false}))
                }
            } else {
                Err(Error::State(
                    "artist Top50 fixture refuses another cohort's path",
                ))
            };
            Some(response)
        }
        PRIVILEGE => {
            let actual = packed(args, "ids");
            let (artist, expected) = ["101", "102", "103"].into_iter().find_map(|artist| {
                let expected = ids(artist)?;
                (expected.map(|id| id.to_string()).as_slice() == actual.as_slice())
                    .then_some((artist, expected))
            })?;
            record(path, args, cache);
            assert!(!cache && treatment);
            assert_eq!(args.as_object().unwrap().len(), 1);
            Some(if artist == "103" {
                Err(Error::Transport("合成歌手热歌权益读取失败".into()))
            } else {
                let data = expected
                    .into_iter()
                    .rev()
                    .map(privilege)
                    .collect::<Vec<_>>();
                Ok(json!({"code":200,"data":data}))
            })
        }
        "/api/artist/detail/dynamic" => {
            ids(args["id"].as_str()?)?;
            record(path, args, cache);
            Some(Err(Error::State(
                "artist playback must not read the details page",
            )))
        }
        "/api/v3/song/detail" => {
            let rows: Vec<Value> = serde_json::from_str(args["c"].as_str()?).ok()?;
            if rows.iter().any(|row| target_song(&row["id"])) {
                record(path, args, cache);
                return Some(Err(Error::State(
                    "artist playback must not rehydrate song rows",
                )));
            }
            None
        }
        PLAYER_URL => {
            let ids = packed(args, "ids");
            if ids.iter().any(|id| target_song(&json!(id))) {
                record(path, args, cache);
            }
            // The normal desktop fixture authorizes and decodes the exact WAV.
            None
        }
        _ => None,
    }
}

fn target_song(id: &Value) -> bool {
    id.as_u64()
        .or_else(|| id.as_str().and_then(|id| id.parse().ok()))
        .is_some_and(|id| (91..=99).contains(&id))
}
