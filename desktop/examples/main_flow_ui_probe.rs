//! A single production WebView/IPC flow against a closed synthetic server.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/main_flow_fixture.rs"]
mod fixture;
#[path = "support/main_flow_wire.rs"]
mod wire;

use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::MusicClient,
    media::integrity::inspect_file,
    protocol::http::{ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

const OWNER: &str = "2";
const TASKS: &str = "download_tasks_v1";
const LEDGER: &str = "offline_music_v1";
const SENTINEL: &[u8] = b"unrelated synthetic file\n";

fn checkpoint(store: &StateStore, media: &Path, seed: &Path) -> Result<Value> {
    let (tasks, records) =
        store.transaction(|tx| Ok((tx.values(OWNER, TASKS)?, tx.values(OWNER, LEDGER)?)))?;
    let (Some((id, task)), Some((_, record))) =
        (tasks.first_key_value(), records.first_key_value())
    else {
        return Err(Error::State("missing published synthetic download"));
    };
    let target = PathBuf::from(
        task["target"]
            .as_str()
            .ok_or(Error::State("missing synthetic path"))?,
    );
    let target = target.canonicalize()?;
    let relative = target
        .strip_prefix(media.canonicalize()?)
        .map_err(|_| Error::State("synthetic download escaped media root"))?;
    let actual = serde_json::to_value(inspect_file(&target)?)?;
    let source = inspect_file(seed)?;
    if tasks.len() != 1
        || records.len() != 1
        || task["status"] != "complete"
        || task["cleanup_pending"] != false
        || task["encrypted"] != false
        || task["output_integrity"] != actual
        || record["integrity"] != actual
        || record["task_id"] != *id
        || record["path"] != json!(target)
        || task["raw_integrity"]["bytes"] != source.bytes
        || task["raw_integrity"]["sha256"] != source.sha256
    {
        return Err(Error::State("synthetic download checkpoint mismatch"));
    }
    Ok(json!({"id":id,"relative":relative,"file":actual,
        "taskIntegrity":task["output_integrity"],"ledgerIntegrity":record["integrity"],
        "source":source}))
}

fn paused(store: &StateStore, id: &Value) -> Result<Value> {
    let (tasks, transfers) = store.transaction(|tx| {
        Ok((
            tx.values(OWNER, TASKS)?,
            tx.values(OWNER, "media_transfers_v1")?,
        ))
    })?;
    let (Some((task_id, task)), Some((_, transfer))) =
        (tasks.first_key_value(), transfers.first_key_value())
    else {
        return Err(Error::State("missing paused task and transfer"));
    };
    let path = transfer["part"]
        .as_str()
        .ok_or(Error::State("missing partial path"))?;
    if tasks.len() != 1
        || transfers.len() != 1
        || id.as_str() != Some(task_id.as_str())
        || task["status"] != "paused"
        || transfer["status"] != "paused"
        || transfer["bytes"] != fixture::media::PREFIX
        || fs::metadata(path)?.len() != fixture::media::PREFIX as u64
        || fs::read(path)? != fixture::media::DATA[..fixture::media::PREFIX]
        || !store.values(OWNER, LEDGER)?.is_empty()
    {
        return Err(Error::State("synthetic pause checkpoint mismatch"));
    }
    Ok(json!({"id":id,"prefixBytes":fixture::media::PREFIX,"prefixVerified":true}))
}

fn disposed(store: &StateStore, media: &Path, checkpoint: &Value) -> Result<Value> {
    let relative = checkpoint["relative"]
        .as_str()
        .ok_or(Error::State("missing download checkpoint"))?;
    let id = checkpoint["id"]
        .as_str()
        .ok_or(Error::State("missing checkpoint task"))?;
    let target = media.join(relative);
    if !matches!(fs::symlink_metadata(&target), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        || fs::read(media.join("unrelated.txt"))? != SENTINEL
    {
        return Err(Error::State(
            "synthetic disposal changed the wrong file or left the target",
        ));
    }
    let (tasks, records, journal) = store.transaction(|tx| {
        Ok((
            tx.values(OWNER, TASKS)?,
            tx.values(OWNER, LEDGER)?,
            tx.values(OWNER, "download_file_operations_v1")?,
        ))
    })?;
    if !tasks.is_empty()
        || !records.is_empty()
        || journal.len() != 1
        || !journal.values().any(|row| {
            row["task_id"] == id && row["action"] == "delete" && row["phase"] == "complete"
        })
    {
        return Err(Error::State(
            "synthetic disposal ledger or journal mismatch",
        ));
    }
    Ok(json!({"id":id,"fileAbsent":true,"ledgerAbsent":true,"sentinelUnchanged":true}))
}

fn main() {
    // SAFETY: precedes WebView and worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    let ordinary_https = fixture::ordinary_https();
    let retry_https = std::env::var("CUMULUS_MAIN_FLOW_HTTPS_RETRY").as_deref() == Ok("1");
    let eof_https = std::env::var("CUMULUS_MAIN_FLOW_HTTPS_EOF").as_deref() == Ok("1");
    let loading_step = std::env::var("CUMULUS_MAIN_FLOW_LOADING_STEP").as_deref() == Ok("1");
    assert!(!(retry_https || eof_https) || ordinary_https);
    assert!(!(retry_https && eof_https));
    assert!(!(loading_step && ordinary_https));
    let reopen = std::env::var("CUMULUS_PROOF_REOPEN").as_deref() == Ok("1");
    if reopen {
        let completed: Value = serde_json::from_slice(
            &fs::read(root.join("first/complete.json")).expect("previous proof completed"),
        )
        .expect("previous completion record");
        assert_eq!(completed["ok"], true);
    } else {
        fs::create_dir(&root).expect("proof root must not exist");
    }
    let root = root.canonicalize().expect("absolute proof root");
    let out = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&out).unwrap();
    let downloads = root.join("media");
    let seed = root.join("synthetic.flac");
    if !reopen {
        fs::create_dir(&downloads).unwrap();
        fs::write(downloads.join("unrelated.txt"), SENTINEL).unwrap();
        fs::write(&seed, fixture::media::DATA).unwrap();
    }
    let path = serde_json::to_string(&downloads).unwrap();
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    use tauri::utils::config::Csp;
    let security = &mut context.config_mut().app.security;
    let Some(Csp::Policy(policy)) = security.csp.take() else {
        panic!("expected application CSP")
    };
    security.csp = Some(Csp::Policy(
        policy.replace(" https://*.music.126.net https://*.music.163.com", ""),
    ));
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let store = Arc::new(StateStore::open(root.join("state"))?);
            let fixture = Arc::new(fixture::Service::new(store.clone(), ordinary_https, retry_https, eof_https, loading_step));
            let wire = Arc::new(if reopen {
                wire::Wire::reopen(fixture.clone())
            } else {
                wire::Wire::new(fixture.clone())
            });
            let audio_path = downloads.clone();
            let audio_fixture = fixture.clone();
            let checkpoint_seed = seed.clone();
            let client = MusicClient::open_with_request_context(
                &root.join("state"), None,
                ClientProfile {
                    user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                    os_version: "synthetic-windows".into(),
                    client_sign: Some("synthetic-main-flow-sign".into()),
                },
                Some(wire.clone()), Some(wire::Wire::context()),
            )?
                .with_download_backend(fixture.clone())
                .with_audio_factory(move || Ok(Box::new(fixture::Audio::open(seed.clone(), audio_path.clone(), audio_fixture.clone())?)));
            app.manage(client);
            app.add_capability(r#"{"identifier":"synthetic-main-flow-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let webview_storage = out.join("webview");
            app.listen("ux-proof", move |event| {
                let Ok(mut value) = serde_json::from_str::<Value>(event.payload()) else { return };
                let Some(stage) = value["stage"].as_str().map(str::to_owned) else { return };
                if reopen && !["ux_reopened", "failed"].contains(&stage.as_str()) { return; }
                if stage == "approve_login" {
                    if wire.approve().is_err() {
                        fs::write(out.join("failed.json"), br#"{"stage":"approve_login","ok":false}"#).unwrap();
                        handle.exit(1);
                    }
                    return;
                }
                if !ordinary_https && stage == "release_loading_seek" {
                    fixture.release_loading_seek();
                    return;
                }
                if eof_https && stage == "wait_https_prefetch" {
                    let fixture = fixture.clone();
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                        while !fixture.second_source_prefetched() && std::time::Instant::now() < deadline {
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        }
                        if fixture.second_source_prefetched() {
                            handle.get_webview_window("main").unwrap().eval("window.__mainFlowHttpsPrefetched=true;").unwrap();
                        }
                    });
                    return;
                }
                if stage == "wait_download_prefix" {
                    let fixture = fixture.clone();
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
                        while !fixture.media.prefix_waiting() && std::time::Instant::now() < deadline {
                            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                        }
                        if fixture.media.prefix_waiting() {
                            handle.get_webview_window("main").unwrap().eval("window.__mainFlowPrefixWaiting=true;").unwrap();
                        }
                    });
                    return;
                }
                if stage == "disconnect" {
                    fixture.offline();
                    handle.get_webview_window("main").unwrap().eval("window.__mainFlowOffline=true;").unwrap();
                    return;
                }
                let ordinary_stage = ["ux_reopened","ux_guest","ux_qr_pending","ux_login_adopted","ux_recovered","ux_daily","ux_search","ux_search_loading_cover","ux_playback","ux_playlist_created","ux_playlist_edited","ux_playlist_song_added","ux_playlist_song_removed","ux_playlist_cleaned","ux_download_paused","ux_download_complete","ux_local_offline_playback","ux_download_disposed","complete","failed"].contains(&stage.as_str());
                if !(ordinary_stage || retry_https && stage == "ux_https_failure" || eof_https && stage == "ux_https_before_eof") { return; }
                let client = handle.state::<MusicClient>();
                let account = client.account().unwrap();
                value["authenticated"] = json!(account.status == SessionStatus::Authenticated);
                value["nativeEpoch"] = json!(account.epoch);
                value["wire"] = wire.facts();
                let audit = match stage.as_str() {
                    "ux_login_adopted" | "ux_reopened" => client.saved_accounts(&account.epoch).and_then(|records| {
                        if records.value.len() != 1 || !records.value[0].current || !records.value[0].auto_login {
                            return Err(Error::State("synthetic profile was not saved and adopted"));
                        }
                        Ok(json!({"savedCurrentProfile":true}))
                    }),
                    "ux_download_paused" => paused(&store, &value["taskId"]),
                    "ux_download_complete" => checkpoint(&store, &downloads, &checkpoint_seed),
                    "ux_download_disposed" => fs::read(out.join("ux_download_complete.json"))
                        .map_err(Error::from).and_then(|bytes| Ok(serde_json::from_slice::<Value>(&bytes)?))
                        .and_then(|verified| disposed(&store, &downloads, &verified["audit"])),
                    _ => Ok(Value::Null),
                };
                let Ok(audit) = audit else {
                    fs::write(out.join("failed.json"), serde_json::to_vec(&json!({"stage":stage,"auditError":audit.unwrap_err().to_string()})).unwrap()).unwrap();
                    handle.exit(1);
                    return;
                };
                value["audit"] = audit;
                value["fixture"] = fixture.facts();
                fs::write(out.join(format!("{stage}.json")), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let acknowledgement = match stage.as_str() {
                    "ux_https_before_eof" => Some("window.__mainFlowBeforeEofRecorded=true;"),
                    "ux_https_failure" => Some("window.__mainFlowHttpsFailureRecorded=true;"),
                    "ux_download_paused" => Some("window.__mainFlowPauseVerified=true;"),
                    "ux_download_complete" => Some("window.__mainFlowDownloadVerified=true;"),
                    "ux_download_disposed" => Some("window.__mainFlowDownloadDisposed=true;"),
                    _ => None,
                };
                if let Some(acknowledgement) = acknowledgement {
                    handle.get_webview_window("main").unwrap().eval(acknowledgement).unwrap();
                }
                if stage == "complete" || stage == "failed" {
                    handle.exit(if stage == "complete" {0} else {1});
                } else if stage.starts_with("ux_") {
                    let handle = handle.clone();
                    let out = out.clone();
                    let handshake = value["captureHandshake"].as_bool() == Some(true);
                    tauri::async_runtime::spawn(async move {
                        if capture_ack::acknowledge(&handle, &out, &stage, handshake).await
                            && stage == "ux_reopened"
                        {
                            handle.exit(0);
                        }
                    });
                }
            });
            tauri::WebviewWindowBuilder::from_config(app, &window_config)?
                .data_directory(webview_storage)
                .inner_size(1120.0, 760.0)
                .build()?;
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if payload.event()==PageLoadEvent::Finished {
                webview.eval(format!("window.__mainFlowDownloadPath={path};window.__mainFlowOrdinaryHttps={ordinary_https};window.__mainFlowHttpsRetry={retry_https};window.__mainFlowHttpsEof={eof_https};window.__mainFlowLoadingStep={loading_step};")).unwrap();
                let source = if reopen {
                    include_str!("main_flow_reopen_probe.js")
                } else {
                    include_str!("main_flow_ui_probe.js")
                };
                if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
                    webview.eval(capture_ack::script(source)).unwrap();
                } else {
                    webview.eval(source).unwrap();
                }
            }
        })
        .run(context).expect("synthetic main flow");
}
