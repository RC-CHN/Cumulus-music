//! Opt-in in-process recommendations and signed-API-compatible chorus fixture.
use cumulus_core::{
    Error, Result,
    protocol::api::{PlaybackChorusPermission, PlaybackChorusRead},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;

const REVISION: &str = "synthetic-fm-chorus-cookie";
const FAMILIAR_SEEDS: &str = r#"["41","42"]"#;
static LOADS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static BATCHES: Mutex<Vec<Vec<Value>>> = Mutex::new(Vec::new());
static SINGLES: AtomicUsize = AtomicUsize::new(0);
static RELEASED: AtomicBool = AtomicBool::new(false);
static RETURNED: AtomicBool = AtomicBool::new(false);
static FAMILIAR: Semaphore = Semaphore::const_new(0);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_FM_CHORUS_PROOF").is_some()
}

pub fn revision() -> Option<String> {
    enabled().then(|| REVISION.into())
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    Some(Ok(match path {
        "/api/v1/radio/get" => {
            let (source, ids) = if args == &json!({"imageFm":1}) {
                ("personal", [1, 11, 12])
            } else if args == &json!({"imageFm":1,"mode":"FAMILIAR","sourceIds":FAMILIAR_SEEDS}) {
                ("familiar", [1, 21, 22])
            } else {
                return Some(Err(Error::InvalidInput("unexpected sealed FM source")));
            };
            LOADS.lock().unwrap().push(source.into());
            let data: Vec<_> = ids.into_iter().map(|id| json!({
                "id":id,"name":if id==1 {"同一首合成漫游歌曲".into()} else {format!("合成漫游 {id}")},
                "ar":[{"id":12,"name":"合成歌手"}],"al":{"id":7,"name":"合成专辑"},
                "dt":30000,"alg":"fixture","fee":0,
            })).collect();
            json!({"code":200,"data":data})
        }
        "/api/music/dislike/get" => json!({"code":200,"data":{"songIdList":[],"artistIdList":[]}}),
        "/api/v3/radio/trash/get" => json!({"code":200,"count":0,"data":[]}),
        "/api/music-vip-membership/client/vip/info" => {
            json!({"code":200,"data":{"userId":2,"now":1793410000000u64}})
        }
        "/api/song/chorus" => {
            SINGLES.fetch_add(1, Ordering::SeqCst);
            return Some(Err(Error::State(
                "FM chorus must not use a generic single request",
            )));
        }
        _ => return None,
    }))
}

pub async fn batch(
    read: PlaybackChorusRead,
    revision: &str,
    permission: &PlaybackChorusPermission<'_>,
) -> Result<Value> {
    if !enabled() || revision != REVISION {
        return Err(Error::CredentialChanged);
    }
    permission()?;
    let (path, payload) = read.into_request()?;
    assert_eq!(path, "/api/song/chorus");
    let ids: Vec<Value> = serde_json::from_str(payload["ids"].as_str().expect("FM fixture IDs"))?;
    assert_eq!(payload.as_object().expect("FM payload").len(), 1);
    let familiar = ids == [json!(1), json!(21), json!(22)];
    assert!(familiar || ids == [json!(1), json!(11), json!(12)]);
    BATCHES.lock().unwrap().push(ids);
    if familiar {
        FAMILIAR
            .acquire()
            .await
            .map_err(|_| Error::Cancelled)?
            .forget();
        permission()?;
        RETURNED.store(true, Ordering::Release);
    }
    Ok(json!({"code":200,"data":[{"id":1,"startTime":if familiar {13000} else {4000}}]}))
}

pub fn release() {
    assert!(!RELEASED.swap(true, Ordering::AcqRel));
    FAMILIAR.add_permits(1);
}

pub fn report() -> Value {
    json!({
        "recommendations":*LOADS.lock().unwrap(),"batches":*BATCHES.lock().unwrap(),
        "singles":SINGLES.load(Ordering::Acquire),
        "familiarReleased":RELEASED.load(Ordering::Acquire),
        "familiarReturned":RETURNED.load(Ordering::Acquire),
    })
}
