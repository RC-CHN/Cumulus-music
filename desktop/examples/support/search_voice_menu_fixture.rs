use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::service::{
        DownloadDestination, DownloadOptions, DownloadOrigin, DownloadPhase, DownloadService,
        DownloadStatus,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static HOLD: AtomicBool = AtomicBool::new(false);
static HELD: AtomicBool = AtomicBool::new(false);
static RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_VOICE_MENU_PROOF").is_some()
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

pub fn control(action: &Value) -> bool {
    match action.as_str() {
        Some("hold") => HOLD.store(true, Ordering::SeqCst),
        Some("release") => RELEASE.notify_one(),
        Some("check") => (),
        _ => panic!("unexpected synthetic voice menu control"),
    }
    HELD.load(Ordering::SeqCst)
}

pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() || path != "/api/resource/commentInfo/list" {
        return None;
    }
    let id = match args["resourceIds"].as_str() {
        Some("[\"401\"]") => "401",
        Some("[\"402\"]") => "402",
        _ => panic!("unexpected synthetic comment identity"),
    };
    assert_eq!(
        *args,
        json!({"resourceType":1,"resourceIds":format!("[\"{id}\"]")})
    );
    assert!(!cached);
    CALLS.lock().unwrap().push(args.clone());
    if HOLD.swap(false, Ordering::SeqCst) {
        HELD.store(true, Ordering::SeqCst);
        RELEASE.notified().await;
        HELD.store(false, Ordering::SeqCst);
    }
    Some(if id == "401" {
        Ok(json!({"code":200,"data":[{"threadId":"A_DJ_1_401","commentCount":54}]}))
    } else {
        Err(Error::Transport("synthetic comment read failed".into()))
    })
}

pub fn seed(root: &Path, session: Arc<AccountSession>, store: &StateStore) -> Result<()> {
    let parent = root.join("episode files");
    std::fs::create_dir(&parent)?;
    let service = DownloadService::new(session)?;
    let mut task = service.create(
        &json!("1"),
        DownloadDestination::Path {
            path: parent.join("synthetic-voice.wav"),
        },
        DownloadOptions::default(),
    )?;
    let DownloadDestination::Path { path } = &task.destination else {
        unreachable!("synthetic destination");
    };
    std::fs::write(path, b"synthetic completed voice fixture")?;
    task.target = Some(path.clone());
    task.output_integrity = Some(cumulus_core::media::integrity::inspect_file(path)?);
    task.origin = DownloadOrigin::Voice {
        voice_id: "401".into(),
    };
    task.status = DownloadStatus::Complete;
    task.phase = DownloadPhase::Complete;
    let mut record = serde_json::to_value(&task)?;
    record["target_parent"] = record["root_identity"].clone();
    store.put("2", "download_tasks_v1", &task.id, &record)
}
