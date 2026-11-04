//! Isolated two-read recommendation fixture. No profile data or API leaves the host.
use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

const MORE: &str = "/api/homepage/more/rcmd/song";
const PRIVILEGE: &str = "/api/song/enhance/privilege";

#[derive(Default)]
struct Fixture {
    hold: AtomicBool,
    wake: Notify,
    requests: Mutex<Vec<Value>>,
    fetches: AtomicUsize,
}

fn fixture() -> &'static Fixture {
    static VALUE: OnceLock<Fixture> = OnceLock::new();
    VALUE.get_or_init(Fixture::default)
}

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_QUEUE_MORE_UI_PROOF").is_some()
}

pub fn tip_enabled() -> bool {
    std::env::var_os("CUMULUS_QUEUE_MORE_TIP_PROOF").is_some()
}

pub fn groups() -> Result<PcExperimentGroups> {
    let group = if std::env::var_os("CUMULUS_QUEUE_MORE_CONTROL").is_some() {
        "c"
    } else {
        "t1"
    };
    PcExperimentGroups::refreshed(&json!({"data":[
        {"expName":"PH-PC-tagT","expGroupName":"c"},
        {"expName":"PH-PC-recommendMore","expGroupName":group}
    ]}))
}

pub fn control(mode: &str) -> Result<Value> {
    let fixture = fixture();
    match mode {
        "hold" => fixture.hold.store(true, Ordering::SeqCst),
        "release" => {
            fixture.hold.store(false, Ordering::SeqCst);
            fixture.wake.notify_waiters();
        }
        "read" => {}
        _ => {
            return Err(Error::InvalidInput(
                "unknown queue recommendation fixture mode",
            ));
        }
    }
    Ok(report())
}

pub fn report() -> Value {
    let fixture = fixture();
    json!({"requests": fixture.requests.lock().unwrap().clone(),
           "held":fixture.hold.load(Ordering::SeqCst)})
}

fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id}"),"ar":[{"id":7,"name":"合成歌手"}],
           "al":{"id":8,"name":"合成专辑"},"dt":if tip_enabled() {45000} else {30000},"fee":0})
}

pub async fn response(path: &str, args: &Value, cache: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let fixture = fixture();
    match path {
        "/api/v3/discovery/recommend/songs" => Some(Ok(json!({
            "code":200,"data":{"dailySongs":[song(1),song(2)]}
        }))),
        MORE => {
            assert_eq!(args["source"], "curlist_more_rcmd");
            assert_eq!(args.as_object().unwrap().len(), 3);
            assert!(args["songIds"].is_array());
            fixture.requests.lock().unwrap().push(json!({
                "path":path,"args":args,"cache":cache
            }));
            let batch = fixture.fetches.fetch_add(1, Ordering::SeqCst);
            let first = 51 + batch as u64 * 10;
            loop {
                let wake = fixture.wake.notified();
                tokio::pin!(wake);
                wake.as_mut().enable();
                if !fixture.hold.load(Ordering::SeqCst) {
                    break;
                }
                wake.await;
            }
            let songs = (first..=first + 3).map(song).collect::<Vec<_>>();
            let alg = (first..=first + 3)
                .map(|id| (id.to_string(), json!("synthetic-more")))
                .collect::<serde_json::Map<_, _>>();
            Some(Ok(
                json!({"code":200,"data":{"songList":songs,"algMap":alg}}),
            ))
        }
        PRIVILEGE => {
            assert_eq!(args.as_object().unwrap().len(), 1);
            let raw = args["ids"].as_str().expect("privilege IDs JSON string");
            let ids: Vec<Value> = serde_json::from_str(raw).expect("privilege IDs array");
            assert_eq!(
                ids.len(),
                3,
                "official upper bound applies before privilege reads"
            );
            fixture.requests.lock().unwrap().push(json!({
                "path":path,"args":args,"cache":cache
            }));
            Some(Ok(
                json!({"code":200,"data": ids.into_iter().map(|id| json!({
                "id":id, "st":0,"fee":0,"cp":1,"fl":320000,"pl":320000,"dl":320000
            })).collect::<Vec<_>>()}),
            ))
        }
        _ => None,
    }
}
