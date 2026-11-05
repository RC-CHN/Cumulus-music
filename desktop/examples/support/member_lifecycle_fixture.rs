//! Isolated automatic membership bootstrap and expiry read with a real search consumer.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{sync::Mutex, time::Instant};

const NOW: u64 = 1_900_000_000_000;
const FIRST_EXPIRY_MS: u64 = 2_000;
const SECOND_EXPIRY_MS: u64 = 4_500;
const ACCOUNT: &str = "/api/w/nuser/account/get";
const MEMBERSHIP: &str = "/api/music-vip-membership/client/vip/info";
const PRIVILEGES: &str = "/api/song/enhance/privilege";
const IDS: &str = r#"["601","602","603","604","605","606"]"#;

static RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static STATE: Mutex<State> = Mutex::new(State {
    calls: Vec::new(),
    membership_reads: 0,
    first_response: None,
    recheck_after_ms: None,
});

struct State {
    calls: Vec<&'static str>,
    membership_reads: u32,
    first_response: Option<Instant>,
    recheck_after_ms: Option<u128>,
}

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_MEMBER_LIFECYCLE_PROOF").is_some()
}

pub fn release() {
    assert_eq!(
        STATE.lock().unwrap().membership_reads,
        1,
        "startup read precedes release"
    );
    RELEASE.notify_one();
}

pub fn report() -> Value {
    let state = STATE.lock().unwrap();
    json!({"calls":state.calls,"membershipReads":state.membership_reads,
        "recheckAfterMs":state.recheck_after_ms,
        "firstExpiryMs":FIRST_EXPIRY_MS,"secondExpiryMs":SECOND_EXPIRY_MS})
}

pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    match path {
        ACCOUNT => {
            STATE.lock().unwrap().calls.push("account_get");
            None // Continue through the ordinary verified synthetic account/get.
        }
        MEMBERSHIP => {
            assert!(!cached);
            assert_eq!(*args, json!({"userId":"2"}));
            let attempt = {
                let mut state = STATE.lock().unwrap();
                assert_eq!(state.calls.first(), Some(&"account_get"));
                state.membership_reads += 1;
                state.calls.push("membership");
                state.membership_reads
            };
            let now = match attempt {
                1 => {
                    RELEASE.notified().await;
                    STATE.lock().unwrap().first_response = Some(Instant::now());
                    NOW
                }
                2 => {
                    let mut state = STATE.lock().unwrap();
                    state.recheck_after_ms =
                        state.first_response.map(|at| at.elapsed().as_millis());
                    NOW + FIRST_EXPIRY_MS + 1_000
                }
                _ => return Some(Err(Error::State("unexpected automatic membership reread"))),
            };
            Some(Ok(json!({"code":200,"data":{"userId":2,"now":now,
                "associator":{"expireTime":NOW+FIRST_EXPIRY_MS,"vipLevel":7},
                "redplus":{"expireTime":NOW+SECOND_EXPIRY_MS,"vipLevel":1}}})))
        }
        PRIVILEGES => {
            assert!(!cached);
            assert_eq!(*args, json!({"ids":IDS}));
            let mut state = STATE.lock().unwrap();
            assert!(state.first_response.is_some());
            let expired = state.membership_reads == 2;
            state.calls.push(if expired {
                "privileges_expired"
            } else {
                "privileges_active"
            });
            let data: Vec<_> = (601..=606)
                .map(|id| {
                    json!({"id":id,"st":if id==601 && expired {-200} else {0},
                    "fee":if id==601 && !expired {1} else {0}})
                })
                .collect();
            Some(Ok(json!({"code":200,"data":data})))
        }
        _ => None,
    }
}
