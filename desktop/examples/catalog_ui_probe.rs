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

#[path = "support/artist_mv_fixture.rs"]
mod artist_mv_fixture;
#[path = "support/collection_fixture.rs"]
mod collection_fixture;
#[path = "support/comment_read_fixture.rs"]
mod comment_read_fixture;
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[path = "support/follow_new_song_fixture.rs"]
mod follow_new_song_fixture;
#[path = "support/history_fixture.rs"]
mod history_fixture;
#[path = "support/catalog_live.rs"]
mod live;
#[path = "support/ordinary_essential_fixture.rs"]
mod ordinary_essential_fixture;
#[path = "support/relay_contention.rs"]
mod relay_contention;
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[path = "support/search_mv_fixture.rs"]
mod search_mv_fixture;
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[cfg(target_os = "linux")]
fn current_locale(category: libc::c_int) -> String {
    // SAFETY: startup-only inspection. The returned pointer is copied before
    // any further locale query; this does not change the native locale.
    unsafe {
        let value = libc::setlocale(category, std::ptr::null());
        assert!(!value.is_null());
        std::ffi::CStr::from_ptr(value)
            .to_string_lossy()
            .into_owned()
    }
}

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
    let artists = std::env::var("CUMULUS_ARTIST_PROOF").as_deref() == Ok("1");
    let artist_mv = std::env::var("CUMULUS_ARTIST_MV_PROOF").as_deref() == Ok("1");
    assert!(
        !artist_mv || !is_live,
        "artist MV proof uses synthetic services only"
    );
    assert!(
        !(artists && is_live),
        "artist proof uses synthetic services only"
    );
    let albums = std::env::var("CUMULUS_ALBUM_PROOF").as_deref() == Ok("1");
    assert!(
        !(albums && is_live),
        "album proof uses synthetic services only"
    );
    let rankings = std::env::var("CUMULUS_RANKINGS_PROOF").as_deref() == Ok("1");
    assert!(
        !(rankings && is_live),
        "ranking proof uses synthetic services only"
    );
    let album_cards = std::env::var("CUMULUS_ALBUM_CARD_PROOF").as_deref() == Ok("1");
    assert!(
        !album_cards || !is_live,
        "album card proof uses isolated synthetic APIs only"
    );
    let artist_cards = std::env::var("CUMULUS_ARTIST_CARD_PROOF").as_deref() == Ok("1");
    assert!(
        !artist_cards || !is_live,
        "artist card proof uses isolated synthetic APIs only"
    );
    let comment_reads = std::env::var("CUMULUS_COMMENT_READ_PROOF").as_deref() == Ok("1");
    assert!(
        !comment_reads || !is_live,
        "comment read proof uses synthetic services only"
    );
    let follow_new_song = std::env::var("CUMULUS_FOLLOW_NEW_SONG_PROOF").as_deref() == Ok("1");
    let follow_offline_head =
        std::env::var("CUMULUS_FOLLOW_OFFLINE_HEAD_PROOF").as_deref() == Ok("1");
    assert!(!follow_offline_head || follow_new_song);
    assert!(
        !follow_new_song || !is_live,
        "followed release proof uses synthetic services only"
    );
    let ordinary_essential =
        std::env::var("CUMULUS_ORDINARY_ESSENTIAL_PROOF").as_deref() == Ok("1");
    assert!(
        !ordinary_essential || !is_live,
        "choice probe uses closed synthetic services"
    );
    let interactive = ordinary_essential
        || albums
        || artists
        || artist_mv
        || rankings
        || album_cards
        || artist_cards
        || comment_reads
        || follow_new_song;
    let collections = std::env::var("CUMULUS_COLLECTION_PROOF").as_deref() == Ok("1");
    let collection_context =
        std::env::var("CUMULUS_COLLECTION_CONTEXT_PROOF").as_deref() == Ok("1");
    assert!(
        !collection_context || (collections && !is_live),
        "collection context proof uses isolated synthetic APIs only"
    );
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
        #[cfg(target_os = "linux")]
        if ordinary_essential {
            let numeric=current_locale(libc::LC_NUMERIC);
            let messages=current_locale(libc::LC_MESSAGES);
            let characters=current_locale(libc::LC_CTYPE);
            assert_eq!(numeric,"C");
            fs::write(output.join("locale.json"),serde_json::to_vec(&serde_json::json!({
                "numeric":numeric,"messages":messages,"characters":characters
            }))?)?;
        }
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let contention=Arc::new(relay_contention::Fixture::new(registry.store()));
        let catalog_fixture=Arc::new(fixture::Library::default());
        let ordinary_essential_fixture=Arc::new(ordinary_essential_fixture::Library::new(catalog_fixture.clone()));
        let artist_mv_fixture=Arc::new(artist_mv_fixture::Library::new(catalog_fixture.clone()));
        if album_cards {catalog_fixture.album_cards();}
        if artist_cards {catalog_fixture.artist_cards();}
        let collection_fixture=Arc::new(collection_fixture::Library::default());
        let comment_fixture=Arc::new(comment_read_fixture::Library::default());
        let follow_new_song_fixture=Arc::new(follow_new_song_fixture::Library::new(collection_fixture.clone(),follow_offline_head));
        let history_fixture=Arc::new(history_fixture::Library::default());
        let settings_fixture=Arc::new(settings_fixture::Library::default());
        let relay_fixture=Arc::new(relay_fixture::Library::default());
        if relay_settings {relay_fixture.mode("setting-off");}
        if relay_recovery {relay_fixture.mode("publication-rejected");}
        let api:Arc<dyn Api>=if let Some(profile)=profile{Arc::new(live::Reads::open(&profile,&output)?)}else if ordinary_essential {ordinary_essential_fixture.clone()}else if artist_mv {artist_mv_fixture.clone()}else if comment_reads {comment_fixture.clone()}else if relays {relay_fixture.clone()}else if settings {settings_fixture.clone()}else if histories {history_fixture.clone()}else if follow_new_song {follow_new_song_fixture.clone()}else if collections{collection_fixture.clone()}else{catalog_fixture.clone()};
        let follow_store=registry.store();
        let session=Arc::new(AccountSession::new(api,Some(registry.store())));
        let publisher=Arc::new(cumulus_core::relay::publisher::RelayPublisher::new(session.clone())?);
        let client=MusicClient::with_session(session.clone(),registry);
        let client=if relay_publication {client.with_relay_publication()}else{client};
        let offline_dir=root.join("follow-offline");
        let follow_file=offline_dir.join("published-head.wav");
        let follow_sample=root.join("relay-synthetic.wav");
        let client=if (relays || interactive || histories) && !is_live {
            let wave=relay_fixture::wave(&root)?;
            if follow_offline_head {fs::create_dir(&offline_dir)?;}
            client.with_audio_factory(move|| {
                let audio=relay_fixture::NativeAudio::open(wave.clone())?;
                Ok(Box::new(if follow_offline_head {audio.allow_download_directory(offline_dir.clone())} else {audio}))
            })
        }else{cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true)};
        let client = if artist_mv {
            let path=search_mv_fixture::create(&root)?;
            client.with_video_factory(Arc::new(move || Ok(Box::new(search_mv_fixture::NativeVideo::open(path.clone())?))))
        } else {client};
        app.manage(client);
        app.add_capability(r#"{"identifier":"ux-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        let focus_directory=root.join("focus-webview");
        app.listen("ux-proof",move |event| {
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("");
            if ordinary_essential && stage=="essential-fixture" {
                let id=value["id"].as_u64().expect("fixture sequence");
                ordinary_essential_fixture.set(value["owner"].as_u64().unwrap(),value["mode"].as_str().unwrap());
                handle.get_webview_window("main").unwrap().eval(format!("window.__essentialAck={id};")).unwrap();
                return;
            }
            if follow_new_song && stage=="follow-new-song-allow-first" {
                if follow_offline_head {
                    follow_new_song_fixture::Library::seed_offline_head(&session,&follow_store,&follow_sample,&follow_file).unwrap();
                }
                follow_new_song_fixture.allow_first_page();
                return;
            }
            if comment_reads && stage=="comment-read-fixture" {
                let id=value["id"].as_u64().expect("comment fixture sequence");let mode=value["mode"].as_str().unwrap();
                assert!(["read","none","fail-next","fail-hot","fail-target","fail-floor-first","fail-floor-next","hold-next","expire","empty","missing-target","resource-offline","hold-resource"].contains(&mode));
                if mode!="read" {comment_fixture.mode(mode);}
                handle.get_webview_window("main").unwrap().eval(format!("window.__commentReadAck[{id}]={};",comment_fixture.report())).unwrap();return;
            }
            if collection_context && stage=="collection-context-fixture" {
                let id=value["id"].as_u64().expect("collection fixture id");
                let mode=value["mode"].as_str().unwrap();
                assert!(["read","none","fail_saved","fail_more","fail_purchase","empty_purchase","empty_saved","fail_follow","empty_follow","delay_more","delay_first","expire"].contains(&mode));
                if mode!="read" {collection_fixture.mode(mode);}
                handle.get_webview_window("main").unwrap().eval(format!("window.__collectionContextAck[{id}]={};",collection_fixture.report())).unwrap();return;
            }
            if album_cards && stage=="album-card-fixture" {
                let id=value["id"].as_u64().expect("album fixture id");let mode=value["mode"].as_str().unwrap();
                assert!(["read","none","album-offline","album-sub-reject","album-sub-lost"].contains(&mode));
                if mode!="read" {catalog_fixture.mode(mode);}
                handle.get_webview_window("main").unwrap().eval(format!("window.__albumCardAck[{id}]={};",catalog_fixture.report())).unwrap();return;
            }
            if artist_cards && stage=="artist-card-fixture" {
                let id=value["id"].as_u64().expect("artist fixture id");let mode=value["mode"].as_str().unwrap();
                assert!(["read","none","artist-state-offline","artist-sub-reject","artist-sub-lost"].contains(&mode));
                if mode!="read" {catalog_fixture.mode(mode);}
                handle.get_webview_window("main").unwrap().eval(format!("window.__artistCardAck[{id}]={};",catalog_fixture.report())).unwrap();return;
            }
            if stage=="relay_contention" && relay_publication && !is_live {
                let contention=contention.clone();let session=session.clone();let fixture=relay_fixture.clone();let handle=handle.clone();
                let id=value["id"].as_u64().expect("contention sequence");
                let action=value["action"].as_str().unwrap().to_owned();
                tauri::async_runtime::spawn(async move {
                    let owner=session.token().unwrap().uid.unwrap();
                    let mut result=contention.command(&owner,&action).await.unwrap_or_else(|error|serde_json::json!({"error":error.to_string()}));
                    result["pulls"]=serde_json::json!(fixture.pulls.load(std::sync::atomic::Ordering::SeqCst));
                    handle.get_webview_window("main").unwrap().eval(format!("window.__relayContentionAck[{id}]={result};")).unwrap();
                });return;
            }
            if stage=="fixture" && interactive && !is_live {
                catalog_fixture.mode(value["mode"].as_str().unwrap_or(""));
                handle.get_webview_window("main").unwrap().eval(format!("window.__CUMULUS_ALBUM_MODE__={};",value["mode"])).unwrap();
                return;
            }
            if stage=="fixture" && collections && !is_live {collection_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="fixture" && histories && !is_live {history_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="fixture" && settings && !is_live {settings_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="fixture" && relays && !is_live {relay_fixture.mode(value["mode"].as_str().unwrap_or(""));return;}
            if stage=="resize" {
                let window=handle.get_webview_window("main").unwrap();
                let width=value["width"].as_f64().unwrap();let height=value["height"].as_f64().unwrap();
                // WebKit's CSS device ratio can differ from GTK's integer scale.
                if let Some(scale)=value["scale"].as_f64() {
                    assert!(scale.is_finite() && scale>0.0 && scale<=4.0);
                    window.set_size(tauri::PhysicalSize::new((width*scale).round() as u32,(height*scale).round() as u32)).unwrap();
                } else {window.set_size(tauri::LogicalSize::new(width,height)).unwrap();}
                return;
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
            let collection_report=collection_context.then(|| collection_fixture.report());
            let album_card_report=(album_cards || artist_cards).then(||catalog_fixture.report());
            let release_report=follow_new_song.then(||follow_new_song_fixture.report());
            let essential_report=ordinary_essential.then(||ordinary_essential_fixture.report());
            let artist_mv_report=artist_mv.then(||artist_mv_fixture.report());
            let follow_store=follow_store.clone();
            let comment_read_report=comment_reads.then(||comment_fixture.report());
            let publication_fixture=relay_fixture.clone();let publication=publisher.clone();let publication_session=session.clone();
            let artist_song_reads=catalog_fixture.artist_song_reads.load(std::sync::atomic::Ordering::SeqCst);
            let artist_album_reads=catalog_fixture.artist_album_reads.load(std::sync::atomic::Ordering::SeqCst);
            let comment_reads=catalog_fixture.comment_reads.load(std::sync::atomic::Ordering::SeqCst);
            let history_writes=history_fixture.writes.load(std::sync::atomic::Ordering::SeqCst);
            let history_reads=history_fixture.reads.load(std::sync::atomic::Ordering::SeqCst);
            let settings_writes=settings_fixture.writes.load(std::sync::atomic::Ordering::SeqCst);
            let relay_pulls=relay_fixture.pulls.load(std::sync::atomic::Ordering::SeqCst);
            let relay_setting_writes=relay_fixture.setting_writes.load(std::sync::atomic::Ordering::SeqCst);
            tauri::async_runtime::spawn(async move {
                let mut value=value;
                if let Some(report)=collection_report {value["collectionFixture"]=report;}
                if let Some(report)=album_card_report {value["albumCardFixture"]=report;}
                if let Some(report)=release_report {value["followNewSongFixture"]=report;}
                if let Some(report)=essential_report {value["essentialReads"]=report;}
                if let Some(report)=artist_mv_report {value["artistMvCalls"]=serde_json::json!(report);value["nativeMvCalls"]=serde_json::json!(search_mv_fixture::calls());}
                if let Some(report)=comment_read_report {value["commentReadFixture"]=report;}
                if artists {value["artistSongReads"]=serde_json::json!(artist_song_reads);value["artistAlbumReads"]=serde_json::json!(artist_album_reads);}
                if albums || artists {value["commentReads"]=serde_json::json!(comment_reads);}
                if histories {value["syntheticHistoryWrites"]=serde_json::json!(history_writes);value["syntheticHistoryReads"]=serde_json::json!(history_reads);}
                if settings {value["syntheticSettingsWrites"]=serde_json::json!(settings_writes);}
                if relays {value["syntheticRelayPulls"]=serde_json::json!(relay_pulls);value["syntheticRelaySettingWrites"]=serde_json::json!(relay_setting_writes);}
                let client=handle.state::<MusicClient>();
                let epoch=client.account().unwrap().epoch;
                if relay_recovery {
                    value["publicationView"]=serde_json::to_value(client.relay_publication_view(&epoch,None).unwrap().value).unwrap();
                    value["publicationCalls"]=serde_json::json!(publication_fixture.publications.lock().unwrap().len());
                    value["networkCalls"]=serde_json::json!(publication_fixture.network_calls.load(std::sync::atomic::Ordering::SeqCst));
                }
                if (is_live || relays || interactive) && value["playing"]==true {
                    let player=client.subscribe_playback(&epoch).await.unwrap().current().unwrap();
                    if player.position.is_none_or(|p|p<=0.2)||player.queue_len==0||player.error.is_some(){
                        let failure=serde_json::json!({"ok":false,"error":"native playback did not advance",
                            "stage":stage,"state":player.state,"position":player.position,
                            "queueLen":player.queue_len,"hasPlaybackError":player.error.is_some()});
                        fs::write(output.join("failed.json"),serde_json::to_vec(&failure).unwrap()).unwrap();handle.exit(1);return;
                    }
                    value["nativePlayer"]=serde_json::to_value(player).unwrap();
                    value["nativeQueue"]=serde_json::to_value(client.player_queue(&epoch).await.unwrap()).unwrap();
                    if follow_new_song {
                        let account=publication_session.token().unwrap();
                        let queue=follow_store.get(account.uid.as_deref().unwrap(),"playback","queue").unwrap().unwrap();
                        value["nativeFollowSources"]=serde_json::json!(queue["items"].as_array().unwrap().iter().map(|item|&item["source"]).collect::<Vec<_>>());
                    }
                }
                if (is_live || relays || interactive) && ["complete","failed"].contains(&stage.as_str()){let _=client.player_action(&epoch,PlayerAction::Stop).await;}
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
                if (interactive || collection_context) && stage.starts_with("ux_") {
                    if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1") {
                        let marker=output.join(format!("{stage}.captured"));
                        let captured=tokio::time::timeout(std::time::Duration::from_secs(10),async {
                            while !marker.is_file() {tokio::time::sleep(std::time::Duration::from_millis(40)).await;}
                        }).await;
                        if captured.is_err(){fs::write(output.join("failed.json"),br#"{"ok":false,"error":"album screenshot acknowledgement timed out"}"#).unwrap();handle.exit(1);return;}
                    }
                    handle.get_webview_window("main").unwrap().eval(format!("window.__CUMULUS_ALBUM_CAPTURE_ACK__={};",serde_json::json!(stage))).unwrap();
                }
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
            view.eval(script.as_deref().unwrap_or(if ordinary_essential {include_str!("ordinary_essential_ui_probe.js")}else if follow_new_song {include_str!("follow_new_song_ui_probe.js")}else if relay_recovery {include_str!("relay_recovery_ui_probe.js")}else if relay_settings {if is_live {include_str!("relay_setting_live_probe.js")}else{include_str!("relay_setting_ui_probe.js")}}else if relays {
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
