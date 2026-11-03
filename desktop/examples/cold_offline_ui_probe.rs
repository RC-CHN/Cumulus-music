//! Separate seed and cold processes exercising production desktop IPC.
#[path = "support/cold_offline_fixture.rs"]
mod fixture;

fn main() {
    use std::{fs, path::PathBuf};

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert_eq!(
        args.len(),
        2,
        "usage: cold_offline_ui_probe --seed|--cold PROOF_ROOT"
    );
    let root = PathBuf::from(&args[1])
        .canonicalize()
        .expect("existing isolated proof root");
    if args[0] == "--seed" {
        assert!(!root.join("state").exists(), "proof state must be new");
        let value = tauri::async_runtime::block_on(fixture::seed(&root)).expect("synthetic seed");
        fs::write(root.join("seed.json"), serde_json::to_vec(&value).unwrap()).unwrap();
        return;
    }
    assert_eq!(args[0], "--cold", "unrecognized proof phase");
    launch_cold(root);
}

#[cfg(not(feature = "custom-protocol"))]
fn launch_cold(_: std::path::PathBuf) {
    panic!("cold WebView proof requires --features custom-protocol");
}

#[cfg(feature = "custom-protocol")]
fn launch_cold(root: std::path::PathBuf) {
    use cumulus_core::{account::SessionStatus, client::MusicClient};
    use serde_json::{Value, json};
    use std::{fs, sync::Arc};
    use tauri::{Listener, Manager, webview::PageLoadEvent};

    let report: Value = serde_json::from_slice(&fs::read(root.join("seed.json")).unwrap()).unwrap();
    let id = report["id"]
        .as_str()
        .expect("synthetic published ID")
        .to_owned();
    let capture = std::env::var("CUMULUS_COLD_SCREENSHOT").as_deref() == Ok("1");
    let bootstrap =
        serde_json::to_string(&json!({"id":id,"title":fixture::TITLE,"capture":capture})).unwrap();

    // SAFETY: precedes WebView creation and this process's worker setup.
    unsafe { cumulus_desktop::prepare_graphics() };
    let mut context = tauri::generate_context!();
    use tauri::utils::config::Csp;
    let security = &mut context.config_mut().app.security;
    let Some(Csp::Policy(policy)) = security.csp.take() else {
        panic!("expected application CSP");
    };
    security.csp = Some(Csp::Policy(
        policy.replace(" https://*.music.126.net https://*.music.163.com", ""),
    ));
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let offline = Arc::new(fixture::Offline::default());
            let audio = offline.clone();
            let client = MusicClient::open_with_http_backend(&root.join("state"), None, offline.clone())?
                .with_download_backend(offline.clone())
                .with_audio_factory(move || Ok(Box::new(fixture::Audio::open(audio.clone())?)));
            assert_eq!(client.account()?.status, SessionStatus::Unknown);
            app.manage(client);
            app.add_capability(r#"{"identifier":"cold-offline-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("cold-offline-proof", move |event| {
                let Ok(mut value) = serde_json::from_str::<Value>(event.payload()) else { return };
                let Some(stage) = value["stage"].as_str() else { return };
                if stage == "panel_ready" && capture {
                    fs::write(root.join("panel-ready"), b"painted synthetic panel\n").unwrap();
                    let root = root.clone();
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let ack = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                            while !root.join("panel-captured").is_file() {
                                tokio::time::sleep(std::time::Duration::from_millis(35)).await;
                            }
                        }).await;
                        if ack.is_ok() {
                            handle.get_webview_window("main").unwrap()
                                .eval("window.__coldOfflineCaptured=true").unwrap();
                        } else {
                            handle.exit(1);
                        }
                    });
                    return;
                }
                if !matches!(stage, "complete" | "failed") {
                    return;
                }
                let success = stage == "complete";
                value["transport"] = offline.facts();
                value["output"] = json!("null");
                value["accountUnknown"] = json!(handle.state::<MusicClient>().account()
                    .is_ok_and(|value| value.status == SessionStatus::Unknown));
                fs::write(root.join("cold.json"), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                handle.exit(if success { 0 } else { 1 });
            });
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                webview.eval(format!("window.__coldOfflineFixture={bootstrap};")).unwrap();
                webview.eval(include_str!("cold_offline_ui_probe.js")).unwrap();
            }
        })
        .run(context).expect("cold offline WebView proof");
}
