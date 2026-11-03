//! A single production WebView/IPC flow against a closed synthetic server.
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/main_flow_fixture.rs"]
mod fixture;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    media::integrity::inspect_file,
    protocol::http::ApiOrigin,
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
    fs::create_dir(&root).expect("proof root must not exist");
    let root = root.canonicalize().expect("absolute proof root");
    let out = root.join("first");
    fs::create_dir(&out).unwrap();
    let downloads = root.join("media");
    fs::create_dir(&downloads).unwrap();
    fs::write(downloads.join("unrelated.txt"), SENTINEL).unwrap();
    let path = serde_json::to_string(&downloads).unwrap();
    let seed = root.join("synthetic.flac");
    fs::write(&seed, fixture::media::DATA).unwrap();
    let mut context = tauri::generate_context!();
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
            let fixture = Arc::new(fixture::Service::default());
            let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
            let store = registry.store();
            let session = Arc::new(AccountSession::new(fixture.clone(), Some(store.clone())));
            let audio_path = downloads.clone();
            let audio_fixture = fixture.clone();
            let checkpoint_seed = seed.clone();
            let client = MusicClient::with_session(session, registry)
                .with_download_backend(fixture.clone())
                .with_audio_factory(move || Ok(Box::new(fixture::Audio::open(seed.clone(), audio_path.clone(), audio_fixture.clone())?)));
            app.manage(client);
            app.add_capability(r#"{"identifier":"synthetic-main-flow-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            let window = app.get_webview_window("main").unwrap();
            window.set_size(tauri::LogicalSize::new(1120.0,760.0))?;
            app.listen("ux-proof", move |event| {
                let Ok(mut value) = serde_json::from_str::<Value>(event.payload()) else { return };
                let Some(stage) = value["stage"].as_str().map(str::to_owned) else { return };
                if stage == "disconnect" {
                    fixture.offline();
                    handle.get_webview_window("main").unwrap().eval("window.__mainFlowOffline=true;").unwrap();
                    return;
                }
                if !["ux_recovered","ux_daily","ux_search","ux_playback","ux_playlist_created","ux_playlist_edited","ux_playlist_cleaned","ux_download_complete","ux_local_offline_playback","ux_download_disposed","complete","failed"].contains(&stage.as_str()) { return; }
                let client = handle.state::<MusicClient>();
                value["authenticated"] = json!(client.account().is_ok_and(|v| v.status == cumulus_core::account::SessionStatus::Authenticated));
                let audit = match stage.as_str() {
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
                        capture_ack::acknowledge(&handle, &out, &stage, handshake).await;
                    });
                }
            });
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if payload.event()==PageLoadEvent::Finished {
                webview.eval(format!("window.__mainFlowDownloadPath={path};")).unwrap();
                let source = include_str!("main_flow_ui_probe.js");
                if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() == Ok("1") {
                    webview.eval(capture_ack::script(source)).unwrap();
                } else {
                    webview.eval(source).unwrap();
                }
            }
        })
        .run(context).expect("synthetic main flow");
}
