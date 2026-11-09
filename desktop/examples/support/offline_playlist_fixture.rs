//! Private test adapter: inject transport loss and hard denial into a sealed account.
use super::mixed_playlist_fixture;
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
};
use tauri::{Listener, Manager};

pub struct Fixture {
    inner: mixed_playlist_fixture::Fixture,
    mode: AtomicU8,
    unavailable: AtomicUsize,
    denied: AtomicUsize,
    cue: std::path::PathBuf,
    cue_bytes: Vec<u8>,
}

impl Fixture {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let inner = mixed_playlist_fixture::Fixture::new(root)?;
        let cue = root.join("media/合成离线歌单.cue");
        let cue_bytes = b"TITLE \"Offline proof\"\nPERFORMER \"Cumulus\"\n"
            .iter()
            .copied()
            .chain(
                "FILE \"合成混排本地曲目.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"Fixture intro\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"Fixture offline CUE\"\nINDEX 01 00:04:00\n".bytes(),
            )
            .collect::<Vec<_>>();
        fs::write(&cue, &cue_bytes)?;
        Ok(Self {
            inner,
            mode: 0.into(),
            unavailable: 0.into(),
            denied: 0.into(),
            cue,
            cue_bytes,
        })
    }

    fn receipt(&self, store: &StateStore) -> cumulus_core::Result<Value> {
        let snapshot = store
            .get("2", "playlist-offline-snapshot-v1", "11")?
            .unwrap_or(Value::Null);
        let text = snapshot.to_string();
        Ok(json!({
            "mode":self.mode.load(Ordering::Acquire),
            "unavailable":self.unavailable.load(Ordering::Acquire),
            "denied":self.denied.load(Ordering::Acquire),
            "cached":!snapshot.is_null(),
            "cachedRows":snapshot["rows"].as_array().map_or(0, Vec::len),
            "cachedWithoutUrls":!text.contains("https://") && !text.contains("http://"),
            "cueUnchanged":fs::read(&self.cue).is_ok_and(|bytes| bytes == self.cue_bytes),
            "mixed":self.inner.receipt(store)?,
        }))
    }
}

impl Api for Fixture {
    fn request<'a>(
        &'a self,
        path: &'a str,
        args: Value,
        authenticated: bool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path == "/api/v6/playlist/detail" && args["id"] == "11" {
                match self.mode.load(Ordering::Acquire) {
                    1 => {
                        self.unavailable.fetch_add(1, Ordering::AcqRel);
                        return Err(Error::Transport("synthetic playlist transport cut".into()));
                    }
                    2 => {
                        self.denied.fetch_add(1, Ordering::AcqRel);
                        return Ok(json!({"code":403}));
                    }
                    _ => {}
                }
            }
            self.inner.request(path, args, authenticated).await
        })
    }
}

pub fn listen(app: &tauri::App, fixture: Arc<Fixture>, store: StateStore, output: &Path) {
    let output = output.to_path_buf();
    let handle = app.handle().clone();
    app.listen("local-proof", move |event| {
        let mut value: Value = serde_json::from_str(event.payload()).expect("offline proof event");
        let stage = value["stage"].as_str().unwrap_or("").to_owned();
        if stage == "mode" {
            let mode = value["mode"]
                .as_u64()
                .filter(|mode| *mode <= 2)
                .expect("fixture mode");
            fixture.mode.store(mode as u8, Ordering::Release);
            handle
                .get_webview_window("main")
                .unwrap()
                .eval(format!("window.__offlineMode={mode};"))
                .unwrap();
            return;
        }
        if ![
            "ux_01_live",
            "ux_02_cached",
            "ux_03_cue",
            "ux_04_retry_offline",
            "ux_05_reconnected",
            "ux_06_denied",
            "ux_07_no_cache",
            "complete",
            "failed",
        ]
        .contains(&stage.as_str())
        {
            return;
        }
        value["nativeReceipt"] = fixture.receipt(&store).expect("offline proof receipt");
        let pending = output.join(format!("{stage}.pending"));
        fs::write(&pending, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        fs::rename(pending, output.join(format!("{stage}.json"))).unwrap();
        if matches!(stage.as_str(), "complete" | "failed") {
            handle.exit(if stage == "complete" { 0 } else { 1 });
        } else if std::env::var_os("CUMULUS_PROOF_CAPTURE_ACK").is_some() {
            let marker = output.join(format!("{stage}.captured"));
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if tokio::time::timeout(std::time::Duration::from_secs(8), async {
                    while !marker.is_file() {
                        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    }
                })
                .await
                .is_ok()
                {
                    handle
                        .get_webview_window("main")
                        .unwrap()
                        .eval(format!("window.__offlineCapture={};", json!(stage)))
                        .unwrap();
                }
            });
        }
    });
}
