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
static REQUESTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_QUALITY_UI_PROOF").is_some()
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
        let denied = DENIED.load(Ordering::SeqCst) && args["level"] == "vivid";
        Ok(
            json!({"code":200,"data":[{"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":if denied {403} else {200},"md5":"fixture","level":"standard","expi":120}]}),
        )
    })())
}
pub fn metadata(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let data = match path {
        "/api/song/enhance/privilege" => {
            let ids: Vec<Value> =
                serde_json::from_str(args["ids"].as_str().expect("fixture ids")).unwrap();
            json!({"code":200,"data":ids.iter().map(|id|json!({"id":id,"flag":(1<<12)|(1<<14)|(1<<16)|(1<<17)|(1<<18)|(1<<21),"maxbr":999000,"fl":128000,"payed":0,"fee":0})).collect::<Vec<_>>()})
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
            json!({"code":200,"data":[{"resourceId":0,"resourceType":args["resourceTypeList"],"canUse":args["resourceTypeList"]==31&&(!trial||state!=2),"userSrc":0,"trialStatus":if trial&&state==1 {2}else{1},"leftTime":if trial&&state==1 {1000}else{60000},"cookie":"SECRET"}]})
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
