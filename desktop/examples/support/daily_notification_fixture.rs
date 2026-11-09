//! Read-only recommendation input for the signed, network-sealed GTK probe.
use cumulus_core::{Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Mutex;

pub const CONFIG_URL: &str = "https://d5.music.126.net/obj/fixture/daily-notification.data";
const DAILY_V3: &str = "/api/v3/discovery/recommend/songs";
const MEDIA: &str = "/api/song/enhance/player/url/v1";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn mode() -> Option<String> {
    std::env::var("CUMULUS_DAILY_NOTIFICATION_PROOF").ok()
}

pub fn enabled() -> bool {
    mode().is_some()
}

pub fn config() -> Value {
    json!({"preload#push":{"day":3,"count":1,"t1":0.025,"desc":["合成的每日推荐"]}})
}

pub fn groups() -> [Value; 4] {
    let startup = if mode().as_deref() == Some("control") {
        "c"
    } else {
        "t1"
    };
    [
        json!({"expName":"PH-PC-push","expGroupName":startup}),
        json!({"expName":"PH-PC-push2","expGroupName":"c"}),
        json!({"expName":"PH-PC-Pop-Visual","expGroupName":"c"}),
        json!({"expName":"PH-PC-Blacklist","expGroupName":"t1"}),
    ]
}

fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id}"),"ar":[{"id":12,"name":"合成歌手"}],
           "al":{"id":7,"name":"合成专辑"},"dt":235000,"fee":0})
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Value> {
    let mode = mode()?;
    let result = match path {
        "/api/user/setting" => {
            assert_eq!(args, &json!({}));
            assert!(!cached);
            let privacy = if mode == "privacy" { 2 } else { 0 };
            Some(json!({"code":200,"setting":{"personalServiceSetting":privacy}}))
        }
        DAILY_V3 => {
            assert_eq!(args, &json!({"limit":30}));
            assert!(!cached);
            Some(json!({"code":200,"data":{"dailySongs":(1..=4).map(song).collect::<Vec<_>>()}}))
        }
        MEDIA => None, // Standard sealed null-audio source.
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    result
}

pub fn report(store: &StateStore) -> Result<Value> {
    let history = store.get("@device", "daily-notification-v1", "history")?;
    let queue = store.get("2", "playback", "queue")?;
    let sources = queue.and_then(|queue| {
        queue["items"].as_array().map(|rows| {
            rows.iter()
                .map(|row| row["source"].clone())
                .collect::<Vec<_>>()
        })
    });
    Ok(json!({"history":history,"calls":*CALLS.lock().unwrap(),"sources":sources}))
}

/// Pure native observations around the startup/delay boundaries. This never
/// refreshes groups, authorizes an offer, activates the window, or plays audio.
pub fn observe(app: tauri::AppHandle, path: std::path::PathBuf) {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    use tauri::Manager;
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut frames = Vec::new();
        let started = tokio::time::Instant::now();
        for offset in [0, 2750, 4250, 4900] {
            tokio::time::sleep_until(started + Duration::from_millis(offset)).await;
            let client = app.state::<cumulus_core::client::MusicClient>();
            let plan = client.daily_notification_startup_plan();
            let Some(window) = app.get_webview_window("main") else {
                return;
            };
            let fullscreen_geometry = window
                .outer_position()
                .ok()
                .zip(window.outer_size().ok())
                .zip(window.current_monitor().ok().flatten())
                .map(|((position, size), monitor)| {
                    position == *monitor.position() && size == *monitor.size()
                });
            frames.push(json!({
                "elapsedMs":started.elapsed().as_millis(),
                "armed":plan.as_ref().ok().and_then(|plan|plan.as_ref()).is_some(),
                "planError":plan.err().map(|error|error.to_string()),
                "focused":window.is_focused().ok(),
                "visible":window.is_visible().ok(),
                "minimized":window.is_minimized().ok(),
                "fullscreen":window.is_fullscreen().ok(),
                "fullscreenGeometry":fullscreen_geometry,
            }));
            std::fs::write(&path, serde_json::to_vec_pretty(&frames).unwrap()).unwrap();
        }
    });
}
