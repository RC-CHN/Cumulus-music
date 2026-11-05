//! Sealed bridge for explicit fixture file opens; the sliders, player controls,
//! snapshots and persisted settings use the real product endpoints.
use super::Proof;
use cumulus_core::{
    account::SessionStatus,
    client::{MusicClient, dto::PlayerAction},
    effects::equalizer::EqualizerPreferences,
    playback::{PlaybackMode, engine::Cancellation},
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::{Listener, Manager};

pub(super) const SCRIPT: &str = include_str!("spatial_ui.js");

pub(super) fn install(app: &tauri::App) -> tauri::Result<()> {
    app.add_capability(r#"{"identifier":"local-spatial-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
    let next_open = AtomicU64::new(1);
    let handle = app.handle().clone();
    app.listen("local-spatial-proof", move |event| {
        let mut value: Value = serde_json::from_str(event.payload()).expect("sealed spatial event");
        if let Some(ordinal) = value["open"].as_u64() {
            assert!(
                ordinal <= 2
                    && next_open
                        .compare_exchange(ordinal, ordinal + 1, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok(),
                "unexpected fixture open"
            );
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let proof = handle.state::<Proof>();
                let client = handle.state::<MusicClient>();
                let result: cumulus_core::Result<()> = async {
                    let account = client.account()?;
                    if account.status == SessionStatus::Authenticated {
                        return Err(cumulus_core::Error::StaleResponse);
                    }
                    if ordinal == 1 {
                        client
                            .player_action(
                                &account.epoch,
                                PlayerAction::Mode {
                                    mode: PlaybackMode::Sequential,
                                },
                            )
                            .await?;
                    }
                    client
                        .play_native_open_files_warm(
                            &account.epoch,
                            vec![proof.file.clone()],
                            Cancellation::default(),
                        )
                        .await?;
                    Ok(())
                }
                .await;
                let reply = json!({"ordinal":ordinal,"ok":result.is_ok(),
                    "error":result.err().map(|issue| issue.to_string())});
                handle
                    .get_webview_window("main")
                    .unwrap()
                    .eval(format!("window.__spatialOpenAck={reply}"))
                    .unwrap();
            });
            return;
        }
        let stage = value["stage"]
            .as_str()
            .expect("sealed spatial stage")
            .to_owned();
        assert!(matches!(
            stage.as_str(),
            "ux_01_stock_saved"
                | "ux_02_hot_paused"
                | "ux_03_master_off"
                | "ux_04_master_on"
                | "ux_05_paused_seek"
                | "ux_06_ended"
                | "ux_07_reset"
                | "complete"
                | "failed"
        ));
        let proof = handle.state::<Proof>();
        let actual = EqualizerPreferences::read(&proof.store, "local-guest").unwrap();
        value["durable"] = json!({"revision":actual.revision,"settings":actual.settings});
        value["native"] = json!(*proof.evidence.lock().unwrap());
        value["fixtureOpens"] = json!(next_open.load(Ordering::Acquire) - 1);
        value["webviewFinished"] = json!(true);
        value["realMusicRequests"] = json!(0);
        value["realAccountWrites"] = json!(0);
        value["deniedOtherRequests"] =
            json!(proof.offline.denied_other_requests.load(Ordering::SeqCst));
        #[cfg(target_os = "linux")]
        {
            value["numericLocaleIsC"] = json!(true);
            value["messagesLocalePreserved"] = json!(true);
        }
        let dir = proof.root.join("spatial");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{stage}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        match stage.as_str() {
            "complete" => handle.exit(0),
            "failed" => handle.exit(1),
            _ => handle
                .get_webview_window("main")
                .unwrap()
                .eval(format!("window.__spatialStageAck={};", json!(stage)))
                .unwrap(),
        }
    });
    Ok(())
}
