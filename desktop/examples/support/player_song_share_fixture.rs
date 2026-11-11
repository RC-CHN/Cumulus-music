//! Sealed player-layout rights and clipboard observation, no live provider.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{sync::Mutex, time::Duration};
use tauri::{Manager, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio::sync::Notify;

pub(super) const REVISION: &str = "synthetic-player-song-share";
const MARKER: &str = "Cumulus synthetic current-song share clipboard marker";
const URL: &str = "https://music.163.com/song?id=2";
const RIGHTS: [i32; 12] = [1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1];
const SONG_SWAP_HOLD: usize = 10;
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static RELEASE: Notify = Notify::const_new();

pub(super) fn enabled() -> bool {
    std::env::var_os("CUMULUS_PLAYER_SONG_SHARE_PROOF").is_some()
}

pub(super) fn annotate(response: &mut Value) {
    let Some(songs) = response["songs"].as_array() else {
        return;
    };
    let rights = songs
        .iter()
        .filter_map(|song| {
            let id = song["id"]
                .as_u64()
                .or_else(|| song["id"].as_str()?.parse().ok())?;
            Some(json!({"id":id,"st":if id == 3 {-200} else {0},"fee":0,"pl":320000}))
        })
        .collect::<Vec<_>>();
    response["privileges"] = json!(rights);
}

pub(super) async fn privilege(
    id: &str,
    revision: &str,
    permission: &(dyn Fn() -> Result<()> + Send + Sync),
) -> Result<Value> {
    if !enabled() || revision != REVISION {
        return Err(Error::InvalidInput(
            "unexpected synthetic player sharing read",
        ));
    }
    permission()?;
    let (sequence, sp) = {
        let mut calls = CALLS.lock().map_err(|_| Error::LockPoisoned)?;
        let expected_id = if calls.len() < SONG_SWAP_HOLD {
            "2"
        } else {
            "1"
        };
        if id != expected_id {
            return Err(Error::InvalidInput(
                "unexpected synthetic player sharing song",
            ));
        }
        let sp = *RIGHTS
            .get(calls.len())
            .ok_or(Error::State("too many player sharing reads"))?;
        calls.push(json!({"path":"/api/song/enhance/privilege","args":{"ids":serde_json::to_string(&[id])?},"cached":false,"sp":sp}));
        (calls.len(), sp)
    };
    if sequence == SONG_SWAP_HOLD {
        tokio::time::timeout(Duration::from_secs(4), RELEASE.notified())
            .await
            .map_err(|_| Error::State("synthetic late share was not released"))?;
    }
    Ok(json!({"code":200,"data":[{"id":id,"sp":sp,"cs":0}]}))
}

pub(super) fn report() -> Value {
    json!(*CALLS.lock().unwrap())
}

pub(super) fn control<R: Runtime>(app: &tauri::AppHandle<R>, stage: &str, value: &Value) -> bool {
    if !enabled() || !stage.starts_with("player-song-share-") {
        return false;
    }
    let main = app.get_webview_window("main").unwrap();
    match stage {
        "player-song-share-read-count" => {
            main.eval(format!(
                "window.__playerSongShareReads={};",
                CALLS.lock().unwrap().len()
            ))
            .unwrap();
        }
        "player-song-share-release" => {
            assert_eq!(CALLS.lock().unwrap().len(), SONG_SWAP_HOLD);
            RELEASE.notify_one();
            main.eval("window.__playerSongShareReleased=true;").unwrap();
        }
        "player-song-share-clipboard" => {
            let app = app.clone();
            let sequence = value["sequence"]
                .as_u64()
                .expect("sealed clipboard sequence");
            let action = match value["action"].as_str() {
                Some("seed") => ("seed", MARKER),
                Some("marker") => ("marker", MARKER),
                Some("song") => ("song", URL),
                _ => panic!("unexpected synthetic clipboard action"),
            };
            tauri::async_runtime::spawn(async move {
                if action.0 == "seed" {
                    app.clipboard().write_text(MARKER).unwrap();
                }
                let matches = app
                    .clipboard()
                    .read_text()
                    .is_ok_and(|text| text == action.1);
                main.eval(format!(
                    "window.__playerSongShareClipboard[{sequence}]={matches};"
                ))
                .unwrap();
            });
        }
        _ => return false,
    }
    true
}
