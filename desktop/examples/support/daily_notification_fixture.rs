//! Read-only recommendation input for the signed, network-sealed GTK probe.
use cumulus_core::{Error, Result, client::MusicClient, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Mutex;

pub const POPUP: &str = "/api/pc/popup/copyright";
pub const DETAILS: &str = "/api/v3/song/detail";
pub const CONFIG_URL: &str = "https://d5.music.126.net/obj/fixture/daily-notification.data";
pub const DAILY_V3: &str = "/api/v3/discovery/recommend/songs";
pub const SETTINGS: &str = "/api/user/setting";
const MEDIA: &str = "/api/song/enhance/player/url/v1";
const COPYRIGHT_PLAYLIST: u64 = 81;
static COPYRIGHT_WIRE: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn mode() -> Option<String> {
    std::env::var("CUMULUS_DAILY_NOTIFICATION_PROOF").ok()
}

pub fn copyright_case() -> Option<String> {
    mode()?.strip_prefix("copyright_").map(str::to_owned)
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
        json!({"expName":"PH-PC-push2","expGroupName":if copyright_case().is_some() {"t1"} else {"c"}}),
        json!({"expName":"PH-PC-Pop-Visual","expGroupName":"c"}),
        json!({"expName":"PH-PC-Blacklist","expGroupName":"t1"}),
    ]
}

fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id}"),"ar":[{"id":12,"name":"合成歌手"}],
           "al":{"id":7,"name":"合成专辑"},"dt":235000,"fee":0})
}

pub fn signed_reply(path: &str, args: &Value) -> Result<Value> {
    let mode = copyright_case().ok_or(Error::State("copyright fixture is not enabled"))?;
    let response = match path {
        POPUP => {
            assert_eq!(args, &json!({}));
            let data = match mode.as_str() {
                "empty" => json!({"songIdList":[],"songNum":0,"returnSongPlaylistId":0}),
                mode if mode.starts_with("no_id_") => json!({"songIdList":[3,5],"songNum":2}),
                _ => {
                    json!({"songIdList":[3,5],"songNum":2,"returnSongPlaylistId":COPYRIGHT_PLAYLIST})
                }
            };
            if mode == "error" {
                json!({"code":500})
            } else {
                json!({"code":200,"data":data})
            }
        }
        DETAILS => {
            assert_eq!(
                args,
                &json!({"c":r#"[{"id":"3","v":0},{"id":"5","v":0}]"#,"trialMode":"-1"})
            );
            let mut first = song(3);
            first["al"]["picUrl"] =
                json!("https://p1.music.126.net/fixture-copyright.jpg?secret=PRIVATE");
            let second = song(5);
            json!({"code":200,"songs":[first,second],"privileges":[{"id":3,"pl":320000},{"id":5,"pl":320000}]})
        }
        _ => return Err(Error::State("unexpected copyright signed endpoint")),
    };
    COPYRIGHT_WIRE.lock().unwrap().push(json!({"path":path,"args":args,"signedPcContextVerified":true,"sameAccountCredential":true,"responseCode":response["code"]}));
    Ok(response)
}

pub fn seed_playback(client: &MusicClient) -> Result<()> {
    if !matches!(copyright_case().as_deref(), Some("play" | "no_id_play")) {
        return Ok(());
    }
    tauri::async_runtime::block_on(async {
        let page = client.daily_songs().await?;
        let revision = page.value.revision.ok_or(Error::StaleResponse)?;
        client
            .play_selection(
                &page.epoch,
                cumulus_core::client::dto::PlaySelection::DailyRecommendations {
                    revision,
                    index: 1,
                    quality: cumulus_core::membership::Quality::Standard,
                },
            )
            .await?;
        Ok(())
    })
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Value> {
    let mode = mode()?;
    let result = match path {
        SETTINGS => {
            assert_eq!(args, &json!({}));
            assert!(!cached);
            let privacy = if matches!(
                mode.as_str(),
                "privacy" | "copyright_play" | "copyright_no_id_play"
            ) {
                2
            } else {
                0
            };
            Some(json!({"code":200,"setting":{"personalServiceSetting":privacy}}))
        }
        DAILY_V3 => {
            assert_eq!(args, &json!({"limit":30}));
            assert!(!cached);
            Some(json!({"code":200,"data":{"dailySongs":(1..=4).map(song).collect::<Vec<_>>()}}))
        }
        MEDIA => None, // Standard sealed null-audio source.
        "/api/v6/playlist/detail" if copyright_case().is_some() && args["id"] == "81" => Some(
            json!({"code":200,"playlist":{"id":COPYRIGHT_PLAYLIST,"userId":2,"name":"合成回归歌单",
                "creator":{"userId":2,"nickname":"合成用户"},"trackCount":2,
                "trackIds":[{"id":3,"v":0},{"id":5,"v":0}],"tracks":[song(3),song(5)]}}),
        ),
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    result
}

pub fn report(store: &StateStore) -> Result<Value> {
    let history = store.get("@device", "daily-notification-v1", "history")?;
    let copyright = store.get("@device", "daily-notification-v1", "copyright-history")?;
    let queue = store.get("2", "playback", "queue")?;
    let sources = queue.and_then(|queue| {
        queue["items"].as_array().map(|rows| {
            rows.iter()
                .map(|row| row["source"].clone())
                .collect::<Vec<_>>()
        })
    });
    Ok(
        json!({"history":history,"copyrightHistory":copyright,"calls":*CALLS.lock().unwrap(),
        "copyrightWire":*COPYRIGHT_WIRE.lock().unwrap(),"sources":sources}),
    )
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
