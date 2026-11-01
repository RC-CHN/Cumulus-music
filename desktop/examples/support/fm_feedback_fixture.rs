//! All mutations stay in this process's synthetic service and isolated store.
use super::fm_domain as domain;
use cumulus_core::{Error, Result, protocol::api::Api};
use serde_json::{Value, json};
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
use tokio::sync::Notify;
static HOLD_REASONS: AtomicBool = AtomicBool::new(false);
static HOLD_WRITE: AtomicBool = AtomicBool::new(false);
static FAIL_REASON: AtomicBool = AtomicBool::new(false);
static REASON_STYLE: AtomicU8 = AtomicU8::new(0);
static REASON_READS: AtomicU64 = AtomicU64::new(0);
static WRITE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
fn wake() -> &'static Notify {
    static WAKE: OnceLock<Notify> = OnceLock::new();
    WAKE.get_or_init(Notify::new)
}
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_FM_FEEDBACK_UI_PROOF").is_some()
}
fn backend() -> &'static domain::Backend {
    static BACKEND: OnceLock<domain::Backend> = OnceLock::new();
    BACKEND.get_or_init(|| {
        let api = domain::Backend::default();
        api.batch(&(101..=140).collect::<Vec<_>>());
        api
    })
}
async fn wait(flag: &AtomicBool) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let notified = wake().notified();
            if !flag.load(Ordering::Acquire) {
                break;
            }
            notified.await;
        }
    })
    .await
    .map_err(|_| Error::State("synthetic feedback hold timed out"))
}
pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if path == "/api/music-vip-membership/client/vip/info" {
        return Some(Ok(
            json!({"code":200,"data":{"userId":2,"now":1793410000000u64}}),
        ));
    }
    if !matches!(
        path,
        "/api/v1/radio/get"
            | "/api/v1/radio/skip"
            | "/api/v3/radio/trash/get"
            | "/api/music/dislike/get"
            | "/api/personalized/dislike/reason"
            | "/api/radio/trash/add"
            | "/api/radio/trash/del/batch"
            | "/api/music-blacklist/add"
            | "/api/music-blacklist/delete"
    ) {
        return None;
    }
    Some(async {
        if path=="/api/personalized/dislike/reason" {
            REASON_READS.fetch_add(1,Ordering::AcqRel); wait(&HOLD_REASONS).await?;
            if FAIL_REASON.swap(false,Ordering::AcqRel) { return Err(Error::Transport("synthetic reasons read failure".into())); }
            let data=match REASON_STYLE.load(Ordering::Acquire) {
                1=>json!([]),
                _=>json!([{"type":"artist","id":"1","reason":format!("屏蔽歌手：{}","一位用于验证长名称和窄窗口排布的合成歌手".repeat(4))},{"type":"fixed","reason":"推荐重复"},{"type":"fixed","reason":"对此歌曲不感兴趣"}]),
            };
            return Ok(json!({"code":200,"data":data}));
        }
        if path=="/api/radio/trash/add" { WRITE_ATTEMPTS.fetch_add(1,Ordering::AcqRel); wait(&HOLD_WRITE).await?; }
        backend().request(path,args.clone(),false).await
    }.await)
}
pub fn control(kind: &str) {
    match kind {
        "hold-reasons" => HOLD_REASONS.store(true, Ordering::Release),
        "release-reasons" => {
            HOLD_REASONS.store(false, Ordering::Release);
            wake().notify_waiters();
        }
        "reason-error" => FAIL_REASON.store(true, Ordering::Release),
        "reason-empty" => REASON_STYLE.store(1, Ordering::Release),
        "reason-normal" => REASON_STYLE.store(0, Ordering::Release),
        "hold-write" => HOLD_WRITE.store(true, Ordering::Release),
        "release-write" => {
            HOLD_WRITE.store(false, Ordering::Release);
            wake().notify_waiters();
        }
        "trash-rejected" => backend().fault("/api/radio/trash/add", domain::Fault::Reject(403)),
        "trash-lost" => backend().fault("/api/radio/trash/add", domain::Fault::Lost),
        "artist-rejected" => {
            backend().fault("/api/music-blacklist/add", domain::Fault::Reject(403))
        }
        "restore-rejected" => {
            backend().fault("/api/radio/trash/del/batch", domain::Fault::Reject(403))
        }
        "restore-lost" => backend().fault("/api/radio/trash/del/batch", domain::Fault::Lost),
        _ => panic!("unknown synthetic FM feedback control"),
    }
}
pub fn report() -> Value {
    json!({"reasonReads":REASON_READS.load(Ordering::Acquire),"trashAttempts":WRITE_ATTEMPTS.load(Ordering::Acquire),"trashPosts":backend().count("/api/radio/trash/add"),"restorePosts":backend().count("/api/radio/trash/del/batch"),"artistPosts":backend().count("/api/music-blacklist/add"),"artistDeletes":backend().count("/api/music-blacklist/delete"),"skipPosts":backend().count("/api/v1/radio/skip"),"remoteTrash":backend().trash.lock().unwrap().clone(),"remoteArtists":backend().blacklist.lock().unwrap().artists.clone()})
}
