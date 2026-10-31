//! Isolated authorization failures and damaged preferences, never real HTTP.
use cumulus_core::{Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
static METADATA_READS: AtomicUsize = AtomicUsize::new(0);
static TRIAL_WRITES: AtomicUsize = AtomicUsize::new(0);
static TRIAL_STATE: AtomicUsize = AtomicUsize::new(0);
static TRIAL_MODE: AtomicUsize = AtomicUsize::new(0);
static DENIED: AtomicBool = AtomicBool::new(false);
static MASTER_DENIED: AtomicBool = AtomicBool::new(false);
static VIVID_GRANTED: AtomicBool = AtomicBool::new(false);
static PARTIAL: AtomicBool = AtomicBool::new(false);
static STANDARD_ONLY: AtomicBool = AtomicBool::new(false);
static METADATA_DELAY: AtomicUsize = AtomicUsize::new(0);
static TRIAL_DELAY: AtomicUsize = AtomicUsize::new(0);
static METADATA_RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static TRIAL_RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static REQUESTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_QUALITY_UI_PROOF").is_some()
}
pub async fn before(path: &str) {
    if !enabled() {
        return;
    }
    let delay = if path == "/api/vipauth/app/auth/set" {
        TRIAL_DELAY.load(Ordering::SeqCst)
    } else if matches!(
        path,
        "/api/song/enhance/privilege"
            | "/api/song/music/detail/get"
            | "/api/music-vip-membership/client/vip/info"
            | "/api/vipauth/app/auth/query"
    ) {
        METADATA_DELAY.load(Ordering::SeqCst)
    } else {
        0
    };
    if delay > 0 {
        let release = if path == "/api/vipauth/app/auth/set" {
            &TRIAL_RELEASE
        } else {
            &METADATA_RELEASE
        };
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(delay as u64),
            release.notified(),
        )
        .await;
    }
}
pub fn response(args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    Some((|| {
        let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().expect("fixture ids"))?;
        REQUESTS
            .lock()
            .unwrap()
            .push(json!({"id":ids[0],"quality":args["level"]}));
        let denied = DENIED.load(Ordering::SeqCst) && args["level"] == "vivid"
            || MASTER_DENIED.load(Ordering::SeqCst) && args["level"] == "jymaster";
        Ok(
            json!({"code":200,"data":[{"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":if denied {403} else {200},"md5":"fixture","level":"standard","expi":120}]}),
        )
    })())
}
pub fn metadata(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if PARTIAL.load(Ordering::SeqCst)
        && (path == "/api/song/music/detail/get"
            || path == "/api/vipauth/app/auth/query" && args["resourceTypeList"] == 31)
    {
        METADATA_READS.fetch_add(1, Ordering::SeqCst);
        return Some(Err(cumulus_core::Error::State(
            "synthetic partial metadata failure",
        )));
    }
    let data = match path {
        "/api/song/enhance/privilege" => {
            let ids: Vec<Value> =
                serde_json::from_str(args["ids"].as_str().expect("fixture ids")).unwrap();
            let standard = STANDARD_ONLY.load(Ordering::SeqCst);
            json!({"code":200,"data":ids.iter().map(|id|json!({"id":id,"flag":if standard {0}else{(1<<12)|(1<<14)|(1<<16)|(1<<17)|(1<<18)|(1<<21)},"maxbr":if standard {128000}else{999000},"fl":128000,"payed":0,"fee":0})).collect::<Vec<_>>()})
        }
        "/api/song/music/detail/get" => {
            json!({"code":200,"data":{"id":args["songId"],"sq":{"size":123456},"hr":{"size":12340000},"url":"https://private/?SECRET"}})
        }
        "/api/music-vip-membership/client/vip/info" => {
            json!({"code":200,"data":{"userId":2,"now":1000000,"associator":{"expireTime":2000000},"cookie":"SECRET"}})
        }
        "/api/vipauth/app/auth/query" => {
            let trial = std::env::var_os("CUMULUS_QUALITY_TRIAL_PROOF").is_some();
            let mut state = TRIAL_STATE.load(Ordering::SeqCst);
            if trial && state == 0 && std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
                state = 1;
            }
            let granted = VIVID_GRANTED.load(Ordering::SeqCst) && args["resourceTypeList"] == 41;
            json!({"code":200,"data":[{"resourceId":0,"resourceType":args["resourceTypeList"],"canUse":granted||args["resourceTypeList"]==31&&(!trial||state!=2),"userSrc":if granted {1}else{0},"trialStatus":if trial&&state==1 {2}else{1},"leftTime":if trial&&state==1 {1000}else{60000},"cookie":"SECRET"}]})
        }
        "/api/vipauth/app/auth/set" => {
            TRIAL_WRITES.fetch_add(1, Ordering::SeqCst);
            if std::env::var_os("CUMULUS_QUALITY_TRIAL_PROOF").is_some() {
                assert_eq!(args, &json!({"resourceId":0,"resourceType":31}));
                TRIAL_STATE.store(1, Ordering::SeqCst);
                return Some(if TRIAL_MODE.load(Ordering::SeqCst) == 1 {
                    Err(cumulus_core::Error::State("synthetic lost trial response"))
                } else {
                    Ok(json!({"code":200,"data":true}))
                });
            }
            return Some(Err(cumulus_core::Error::State(
                "trial writes forbidden in quality read proof",
            )));
        }
        "/api/vipnewcenter/app/viptab/module/get"
        | "/api/vipnewcenter/app/resource/centertop/carousel/list" => json!({"code":200,"data":[]}),
        _ => return None,
    };
    METADATA_READS.fetch_add(1, Ordering::SeqCst);
    Some(Ok(data))
}
pub fn control(store: &StateStore, value: &Value) {
    match value["kind"].as_str().expect("fixture kind") {
        "deny" => DENIED.store(true, Ordering::SeqCst),
        "allow" => DENIED.store(false, Ordering::SeqCst),
        "trial-expire" => TRIAL_STATE.store(2, Ordering::SeqCst),
        "trial-eligible" => TRIAL_STATE.store(0, Ordering::SeqCst),
        "trial-lost-response" => TRIAL_MODE.store(1, Ordering::SeqCst),
        "trial-positive-response" => TRIAL_MODE.store(0, Ordering::SeqCst),
        "grant-vivid" => VIVID_GRANTED.store(true, Ordering::SeqCst),
        "deny-master" => MASTER_DENIED.store(true, Ordering::SeqCst),
        "allow-master" => MASTER_DENIED.store(false, Ordering::SeqCst),
        "rights-partial" => PARTIAL.store(true, Ordering::SeqCst),
        "rights-full" => PARTIAL.store(false, Ordering::SeqCst),
        "assets-standard" => STANDARD_ONLY.store(true, Ordering::SeqCst),
        "assets-all" => STANDARD_ONLY.store(false, Ordering::SeqCst),
        "metadata-delay" => METADATA_DELAY.store(5000, Ordering::SeqCst),
        "metadata-fast" => {
            METADATA_DELAY.store(0, Ordering::SeqCst);
            METADATA_RELEASE.notify_waiters();
        }
        "trial-delay" => TRIAL_DELAY.store(5000, Ordering::SeqCst),
        "trial-fast" => {
            TRIAL_DELAY.store(0, Ordering::SeqCst);
            TRIAL_RELEASE.notify_waiters();
        }
        "trial-damage" => store
            .put("2", "quality-trials-v1", "sky", &json!({"invalid":true}))
            .unwrap(),
        "trial-restore" => store.delete("2", "quality-trials-v1", "sky").unwrap(),
        "damage" => store
            .put(
                "2",
                "playback-quality-v1",
                "current",
                &json!({"quality":"invalid"}),
            )
            .unwrap(),
        _ => panic!("unknown quality fixture operation"),
    }
}
pub fn report() -> Value {
    json!({"requests":*REQUESTS.lock().unwrap(),"denied":DENIED.load(Ordering::SeqCst),"metadataReads":METADATA_READS.load(Ordering::SeqCst),"trialWrites":TRIAL_WRITES.load(Ordering::SeqCst)})
}
