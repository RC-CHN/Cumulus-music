//! Production UI inspection. Synthetic by default; live mode requires an
//! explicit existing PC profile and enforces a read-only transport allowlist.
#[path = "support/assets.rs"]
mod assets;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::PlayerAction},
    protocol::{api::Api, http::ApiOrigin},
};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

#[path = "support/collection_fixture.rs"]
mod collection_fixture;
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[path = "support/history_fixture.rs"]
mod history_fixture;
#[path = "support/catalog_live.rs"]
mod live;
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
fn main() {
    // SAFETY: first operation before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    let output = root.join(if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
        "reopen"
    } else {
        "first"
    });
    cumulus_core::storage::private_directory_builder()
        .create(&root)
        .expect("new private proof root");
    fs::create_dir(&output).expect("new evidence directory");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let profile = std::env::var_os("CUMULUS_CATALOG_PROFILE").map(PathBuf::from);
    let is_live = profile.is_some();
    let collections = std::env::var("CUMULUS_COLLECTION_PROOF").as_deref() == Ok("1");
    let histories = std::env::var("CUMULUS_HISTORY_PROOF").as_deref() == Ok("1");
    let settings = std::env::var("CUMULUS_SETTINGS_PROOF").as_deref() == Ok("1");
    let relay_settings = std::env::var("CUMULUS_RELAY_SETTING_PROOF").as_deref() == Ok("1");
    let relay_recovery = std::env::var("CUMULUS_RELAY_RECOVERY_PROOF").as_deref() == Ok("1");
    let relay_publication =
        relay_recovery || std::env::var("CUMULUS_RELAY_PUBLICATION_PROOF").as_deref() == Ok("1");
    assert!(
        !(relay_publication && is_live),
        "publication proof uses synthetic services only"
    );
    let relays = relay_publication
        || relay_settings
        || std::env::var("CUMULUS_RELAY_PROOF").as_deref() == Ok("1");
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let collection_fixture=Arc::new(collection_fixture::Library::default());
        let history_fixture=Arc::new(history_fixture::Library::default());
        let settings_fixture=Arc::new(settings_fixture::Library::default());
        let relay_fixture=Arc::new(relay_fixture::Library::default());
        if relay_settings {relay_fixture.mode("setting-off");}
        if relay_recovery {relay_fixture.mode("publication-rejected");}
        let api:Arc<dyn Api>=if let Some(profile)=profile{Arc::new(live::Reads::open(&profile,&output)?)}else if relays {relay_fixture.clone()}else if settings {settings_fixture.clone()}else if histories {history_fixture.clone()}else if collections{collection_fixture.clone()}else{Arc::new(fixture::Library::default())};
        let session=Arc::new(AccountSession::new(api,Some(registry.store())));
        let publisher=Arc::new(cumulus_core::relay::publisher::RelayPublisher::new(session.clone())?);
        let client=MusicClient::with_session(session.clone(),registry);
        let client=if relay_publication {client.with_relay_publication()}else{client};
        let client=if relays && !is_live {let wave=relay_fixture::wave(&root)?;client.with_audio_factory(move||Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?)))}else{cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true)};
        app.manage(client);
        app.add_capability(r#"{"identifier":"ux-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        let focus_directory=root.join("focus-webview");
        app.listen("ux-proof",move |event| {
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("");
            if stage=="fixture" && collections && !is_live {collection_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="fixture" && histories && !is_live {history_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="fixture" && settings && !is_live {settings_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="fixture" && relays && !is_live {relay_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="resize" {
                handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(value["width"].as_f64().unwrap(),value["height"].as_f64().unwrap())).unwrap();return;
            }
            if stage=="pointer" && !is_live {
                handle.get_webview_window("main").unwrap().set_cursor_position(tauri::LogicalPosition::new(value["x"].as_f64().unwrap(),value["y"].as_f64().unwrap())).unwrap();return;
            }
            if stage=="window_focus" && relays && !is_live {
                let outside=value["outside"]==true;
                let app=handle.clone();
                let directory=focus_directory.clone();
                // WebView2 creation must be outside the synchronous event handler.
                tauri::async_runtime::spawn(async move {
                    if outside {
                        let other=tauri::WebviewWindowBuilder::new(&app,"focus-proof",tauri::WebviewUrl::External("about:blank".parse().unwrap())).title("Synthetic focus check").data_directory(directory).inner_size(180.0,80.0).position(8.0,8.0).build().unwrap();
                        other.set_focus().unwrap();
                    } else {
                        app.get_webview_window("main").unwrap().set_focus().unwrap();
                        if let Some(other)=app.get_webview_window("focus-proof"){other.close().unwrap();}
                    }
                });return;
            }
            if !(["complete","failed"].contains(&stage) || stage.starts_with("ux_") && stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')){return;}
            let handle=handle.clone();let output=output.clone();let stage=stage.to_owned();
            let publication_fixture=relay_fixture.clone();let publication=publisher.clone();let publication_session=session.clone();
            let history_writes=history_fixture.writes.load(std::sync::atomic::Ordering::SeqCst);
            let settings_writes=settings_fixture.writes.load(std::sync::atomic::Ordering::SeqCst);
            let relay_pulls=relay_fixture.pulls.load(std::sync::atomic::Ordering::SeqCst);
            let relay_setting_writes=relay_fixture.setting_writes.load(std::sync::atomic::Ordering::SeqCst);
            tauri::async_runtime::spawn(async move {
                let mut value=value;
                if histories {value["syntheticHistoryWrites"]=serde_json::json!(history_writes);}
                if settings {value["syntheticSettingsWrites"]=serde_json::json!(settings_writes);}
                if relays {value["syntheticRelayPulls"]=serde_json::json!(relay_pulls);value["syntheticRelaySettingWrites"]=serde_json::json!(relay_setting_writes);}
                let client=handle.state::<MusicClient>();
                let epoch=client.account().unwrap().epoch;
                if relay_recovery {
                    value["publicationView"]=serde_json::to_value(client.relay_publication_view(&epoch,None).unwrap().value).unwrap();
                    value["publicationCalls"]=serde_json::json!(publication_fixture.publications.lock().unwrap().len());
                    value["networkCalls"]=serde_json::json!(publication_fixture.network_calls.load(std::sync::atomic::Ordering::SeqCst));
                }
                if (is_live || relays) && value["playing"]==true {
                    let player=client.subscribe_playback(&epoch).await.unwrap().current().unwrap();
                    if player.position.is_none_or(|p|p<=0.2)||player.queue_len==0||player.error.is_some(){
                        fs::write(output.join("failed.json"),br#"{"ok":false,"error":"native playback did not advance"}"#).unwrap();handle.exit(1);return;
                    }
                    value["nativePlayer"]=serde_json::to_value(player).unwrap();
                    value["nativeQueue"]=serde_json::to_value(client.player_queue(&epoch).await.unwrap()).unwrap();
                }
                if (is_live || relays) && ["complete","failed"].contains(&stage.as_str()){let _=client.player_action(&epoch,PlayerAction::Stop).await;}
                if relay_publication && stage=="complete" {
                    let token=publication_session.token().unwrap();
                    let settled=tokio::time::timeout(std::time::Duration::from_secs(5),async {
                        loop {let status=publication.status_for(&token).unwrap();if status.pending==0 && status.blocked.is_none(){break;}tokio::time::sleep(std::time::Duration::from_millis(50)).await;}
                    }).await;
                    if settled.is_err(){fs::write(output.join("failed.json"),br#"{"ok":false,"error":"native publication did not settle"}"#).unwrap();handle.exit(1);return;}
                    value["syntheticRelayPublications"]=serde_json::json!(publication_fixture.publications.lock().unwrap().clone());
                    value["publicationPhases"]=serde_json::json!(publication.intents_for(&token).unwrap().into_iter().map(|r|r.phase).collect::<Vec<_>>());
                }
                fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                if ["complete","failed"].contains(&stage.as_str()){handle.exit(if stage=="complete" {0} else {1});}
            });
        });
        // Absolute config dataDirectory values are ignored by Tauri's safe-path
        // resolver. The builder API owns this isolated test directory explicitly.
        tauri::WebviewWindowBuilder::from_config(app, &window_config)?
            .data_directory(root.join("webview"))
            .build()?;
        Ok(())
    }).on_page_load(move |view, payload| {
        if payload.event()==PageLoadEvent::Finished && view.label()=="main" {
            if relay_publication {view.eval("window.__CUMULUS_PUBLICATION_PROOF__=true;").unwrap();}
            let script=std::env::var_os("CUMULUS_UX_SCRIPT").map(|p|fs::read_to_string(p).unwrap());
            view.eval(script.as_deref().unwrap_or(if relay_recovery {include_str!("relay_recovery_ui_probe.js")}else if relay_settings {if is_live {include_str!("relay_setting_live_probe.js")}else{include_str!("relay_setting_ui_probe.js")}}else if relays {
                include_str!("relay_ui_probe.js")
            } else if settings {
                if is_live {include_str!("settings_live_probe.js")}else{include_str!("settings_ui_probe.js")}
            } else if histories {
                if is_live {include_str!("history_live_probe.js")}else{include_str!("history_ui_probe.js")}
            } else if collections {
                if is_live {include_str!("collection_live_probe.js")} else {include_str!("collection_ui_probe.js")}
            } else if is_live{include_str!("catalog_live_probe.js")}else{include_str!("catalog_ui_probe.js")})).unwrap();
        }
    }).run(context).expect("UX probe");
}
