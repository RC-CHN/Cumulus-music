//! Production UI/IPC inspection with isolated synthetic account and preferences.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/download_page_fixture.rs"]
mod download_page_fixture;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[path = "support/intelligence_start_fixture.rs"]
mod intelligence_start_fixture;
#[path = "support/intelligence_ui_fixture.rs"]
mod intelligence_ui_fixture;
#[path = "support/library_fixture.rs"]
mod library_fixture;
#[path = "support/like_recovery_fixture.rs"]
mod like_recovery_fixture;
#[path = "support/media_shortcuts.rs"]
mod media_shortcuts;
#[path = "support/mode_fixture.rs"]
mod mode_fixture;
#[path = "support/playback_quality_fixture.rs"]
mod playback_quality_fixture;
#[path = "support/playlist_collect_fixture.rs"]
mod playlist_collect_fixture;
#[path = "support/playlist_cover_layout_fixture.rs"]
mod playlist_cover_layout_fixture;
#[path = "support/playlist_cover_transfer_fixture.rs"]
mod playlist_cover_transfer_fixture;
#[path = "support/playlist_editor_fixture.rs"]
mod playlist_editor_fixture;
#[path = "support/playlist_layout_fixture.rs"]
mod playlist_layout_fixture;
#[path = "support/queue_collection_fixture.rs"]
mod queue_collection_fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[path = "support/search_fixture.rs"]
mod search_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[path = "support/volume_fixture.rs"]
mod volume_fixture;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

struct Library;
fn song(id: u64) -> Value {
    if id == 1 && std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some() {
        return json!({"id":id,"name":"沿着海岸走到日落之后 · 用于窄窗口排版检查的长歌名（现场特别收录版本）".repeat(2),"ar":[{"name":"SyntheticArtistWithoutWordBreaks".repeat(4)}],"al":{"name":"合成专辑 · 一个很长很长的副标题与发行版本说明".repeat(4)},"dt":6_000_000,"fee":1});
    }
    json!({"id":id,"name":format!("合成歌曲 {id} · A quiet afternoon"),"ar":[{"name":"Cumulus"}],"al":{"name":"界面体验验证"},"dt":if id==1 {6_000_000} else {235_000},"fee":id%3})
}
fn playlist(id: u64) -> Value {
    playlist_editor_fixture::decorate(
        playlist_layout_fixture::decorate(
            json!({"id":id,"userId":if id<20 {2} else {3},"name":if id==1 {"我喜欢的音乐".into()} else if id==2 {"午后散步 · 一份用来检查较长名称与编辑入口的歌单".into()} else {format!("合成歌单 {id}")},"creator":{"userId":if id<20 {2} else {3},"nickname":"Cumulus"},"specialType":if id==1 {5} else {0},"privacy":10,"trackCount":40,"description":"沿着熟悉的小路，留一点时间给音乐。这是一段较长的合成简介，用来检查窄窗口里的文字、封面和操作入口。","tags":["轻音乐"],"trackIds":(1..=40).map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()}),
            id,
        ),
        id,
    )
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(response) = intelligence_start_fixture::response(path, &args).await {
                return response;
            }
            playback_quality_fixture::before(path).await;
            if let Some(response) = mode_fixture::response(path, &args).await {
                return response;
            }
            if let Some(result) = playback_quality_fixture::metadata(path, &args) {
                return result;
            }
            if let Some(result) = download_page_fixture::response(path, &args) {
                return result;
            }
            if let Some(result) = library_fixture::response(path, &args).await {
                return result;
            }
            if let Some(result) = playlist_collect_fixture::response(path, &args).await {
                return result;
            }
            if let Some(result) = queue_collection_fixture::response(path, &args) {
                return result;
            }
            if let Some(result) = search_fixture::response(path, &args) {
                return result;
            }
            if download_page_fixture::enabled()
                && matches!(
                    path,
                    "/api/artist/v3/detail"
                        | "/api/artist/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                return fixture::Library::default().request(path, args, false).await;
            }
            if search_fixture::enabled()
                && matches!(
                    path,
                    "/api/artist/v3/detail"
                        | "/api/album/v3/detail"
                        | "/api/album/privilege"
                        | "/api/album/detail/dynamic"
                        | "/api/song/enhance/privilege"
                )
            {
                if search_fixture::card_failed() {
                    return Err(cumulus_core::Error::Transport("合成卡片读取失败".into()));
                }
                return fixture::Library::default().request(path, args, false).await;
            }
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"界面体验验证"}})
                }
                "/api/user/playlist" => {
                    // One collected list exercises a menu whose reorder items
                    // are both disabled, without changing production state.
                    let last = if playlist_layout_fixture::enabled() {
                        20
                    } else {
                        24
                    };
                    json!({"code":200,"more":false,"playlist":(1..=last).map(playlist).collect::<Vec<_>>()})
                }
                "/api/song/like/get" => json!({"code":200,"ids":[3,7,10]}),
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=40).map(song).collect::<Vec<_>>()}})
                }
                "/api/v6/playlist/detail" => {
                    let id = args["id"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| args["id"].as_u64())
                        .unwrap_or(2);
                    playlist_editor_fixture::wait_while("hold_read").await;
                    if playlist_editor_fixture::fails_read(id) {
                        return Err(cumulus_core::Error::Response(
                            "synthetic playlist editor read failure",
                        ));
                    }
                    if playlist_layout_fixture::first_read_failure(id) {
                        playlist_layout_fixture::wait_for_release().await;
                        json!({"code":500})
                    } else {
                        json!({"code":200,"playlist":playlist(id)})
                    }
                }
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    let mut response = json!({"code":200,"songs":rows.iter().map(|row|row["id"].as_str().and_then(|s|s.parse().ok()).or_else(||row["id"].as_u64()).unwrap()).filter(|id| !search_fixture::missing_song() || *id != 2).map(song).collect::<Vec<_>>()});
                    if intelligence_start_fixture::enabled() {
                        response["privileges"]=json!(rows.iter().map(|row|json!({"id":row["id"],"pl":320000,"maxbr":999000,"flag":0})).collect::<Vec<_>>());
                    }
                    response
                }
                "/api/batch" if playlist_editor_fixture::enabled() => {
                    playlist_editor_fixture::wait_while("hold_write").await;
                    return playlist_editor_fixture::batch(args);
                }
                "/api/nos/token/alloc" if playlist_cover_layout_fixture::enabled() => {
                    return playlist_cover_layout_fixture::allocation().await;
                }
                "/api/playlist/catalogue" => {
                    if playlist_editor_fixture::enabled() {
                        return playlist_editor_fixture::catalogue().await;
                    }
                    json!({"code":200,"categories":{"0":"风格"},"sub":[{"name":"轻音乐","category":0},{"name":"摇滚","category":0}]})
                }
                "/api/cloudsearch/pc" => {
                    json!({"code":200,"result":{"songs":(1..=12).map(song).collect::<Vec<_>>(),"songCount":12}})
                }
                "/api/song/enhance/player/url/v1" => {
                    if let Some(value) = playback_quality_fixture::response(&args) {
                        return value;
                    }
                    let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    json!({"code":200,"data":[{"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/song/lyric/v1" => {
                    let long = std::env::var_os("CUMULUS_PLAYER_LAYOUT_PROOF").is_some();
                    json!({"code":200,"lrc":{"lyric":(0..30).map(|i|format!("[00:{i:02}.00]合成歌词 {i} {}\n", if long && i % 3 == 0 {"SyntheticLyricsWithoutWordBreaks".repeat(4)} else {String::new()})).collect::<String>()}})
                }
                _ => {
                    return Err(cumulus_core::Error::Response(
                        "unsupported UX fixture endpoint",
                    ));
                }
            })
        })
    }
}
fn main() {
    // SAFETY: first operation before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    let motion_proof = std::env::var_os("CUMULUS_MOTION_PROOF").is_some();
    let recovery_proof = std::env::var_os("CUMULUS_LIKE_RECOVERY_PROOF").is_some();
    let media_proof = std::env::var_os("CUMULUS_SYSTEM_MEDIA_PROOF").is_some();
    let shortcut_proof = std::env::var_os("CUMULUS_SHORTCUT_UI_PROOF").is_some();
    let output = root.join(
        if motion_proof && std::env::var_os("CUMULUS_MOTION_NATIVE_REDUCED").is_some() {
            "native-reduced"
        } else if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
            "reopen"
        } else {
            "first"
        },
    );
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&output).expect("new evidence directory");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let proof_store=registry.store();
        search_fixture::initialize(&proof_store)?;
        let recovery = recovery_proof.then(||Arc::new(like_recovery_fixture::Library::new(Arc::new(Library))));
        let api:Arc<dyn Api>=recovery.as_ref().map(|f|f.clone() as Arc<dyn Api>).unwrap_or_else(||Arc::new(Library));
        let transfer=playlist_cover_transfer_fixture::enabled().then(||Arc::new(playlist_cover_transfer_fixture::Fixture::new(api.clone(),proof_store.clone(),root.clone())));
        let api=transfer.as_ref().map(|f|f.clone() as Arc<dyn Api>).unwrap_or(api);
        let session=Arc::new(AccountSession::new(api,Some(registry.store())));
        let download_page=download_page_fixture::enabled().then(||Arc::new(download_page_fixture::Fixture::new(session.clone(),proof_store.clone(),root.clone())));
        let fixture_session=session.clone();
        let mut client=MusicClient::with_session(session,registry);
        if download_page.is_some() { client=client.with_download_backend(Arc::new(download_page_fixture::NoNetwork))
            .with_video_factory(Arc::new(|| Ok(Box::new(cumulus_core::playback::engine::MpvVideoEngine::open(cumulus_core::playback::engine::MpvConfig {null_output:true,..Default::default()},640,360)?)))); }
        if let Some(fixture)=transfer.as_ref() { client=client.with_playlist_cover_backend(fixture.clone()); }
        else if playlist_cover_layout_fixture::enabled() {
            client=client.with_playlist_cover_backend(Arc::new(playlist_cover_layout_fixture::NoNetwork));
        }
        let startup_delay: u64 = std::env::var("CUMULUS_PLAYER_START_DELAY_MS").ok()
            .map(|value| value.parse().expect("integer fixture startup delay")).unwrap_or(0);
        assert!(startup_delay <= 5000, "bounded synthetic startup delay");
        let factory_elapsed = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let factory_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        if motion_proof {
            let elapsed = factory_elapsed.clone();
            let count=factory_count.clone();
            let wave = root.join("relay-synthetic.wav");
            let wave = if wave.exists() { wave } else { relay_fixture::wave(&root)? };
            let download_directory = download_page_fixture::enabled().then(||root.join("media"));
            client = client.with_audio_factory(move || {
                let start = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(startup_delay));
                let mut audio = relay_fixture::NativeAudio::open(wave.clone())?;
                if let Some(directory)=download_directory.as_ref() { audio=audio.allow_download_directory(directory.clone()); }
                elapsed.store(start.elapsed().as_millis() as u64, std::sync::atomic::Ordering::Release);
                count.fetch_add(1,std::sync::atomic::Ordering::AcqRel);
                Ok(Box::new(audio))
            });
        }
        app.manage(if motion_proof { client } else { cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true) });
        app.add_capability(r#"{"identifier":"ux-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        let cover_root=root.clone();
        app.listen("ux-proof",move |event| {
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            if intelligence_start_fixture::enabled() {value["syntheticIntelligenceStart"]=intelligence_start_fixture::report();}
            if search_fixture::enabled() { value["syntheticSearchCalls"]=json!(search_fixture::calls()); }
            let stage=value["stage"].as_str().unwrap_or("");
            if intelligence_start_fixture::enabled() && stage=="intelligence-start-fixture" {
                let kind=value["kind"].as_str().unwrap();intelligence_start_fixture::control(kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__intelligenceStartAck={};",json!(kind))).unwrap();return;
            }
            if intelligence_ui_fixture::enabled() && stage=="intelligence-fixture" {
                let handle=handle.clone();let store=proof_store.clone();let session=fixture_session.clone();let root=cover_root.clone();
                tauri::async_runtime::spawn(async move {
                    let result=intelligence_ui_fixture::control(&handle.state::<MusicClient>(),session,store,&root,value["kind"].as_str().unwrap()).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__intelligenceFixtureAck={ack};")).unwrap();
                });return;
            }
            if (search_fixture::enabled() || std::env::var_os("CUMULUS_SIDEBAR_UI_PROOF").is_some() || std::env::var_os("CUMULUS_PLAYLIST_MENU_UI_PROOF").is_some()) && ["search-pointer", "sidebar-pointer"].contains(&stage) {
                let window=handle.get_webview_window("main").unwrap();
                let position=tauri::LogicalPosition::new(value["x"].as_f64().unwrap(),value["y"].as_f64().unwrap());
                window.set_cursor_position(position).unwrap();
                window.eval("window.__searchPointerAck=true;").unwrap();return;
            }
            if (search_fixture::enabled() || std::env::var_os("CUMULUS_PLAYLIST_MENU_UI_PROOF").is_some()) && stage=="search-clipboard-check" {
                use tauri_plugin_clipboard_manager::ClipboardExt;
                let handle=handle.clone(); let expected=value["expected"].as_str().unwrap().to_owned();
                tauri::async_runtime::spawn(async move {
                    let matches = handle.clipboard().read_text().is_ok_and(|text|text==expected);
                    handle.get_webview_window("main").unwrap().eval(format!("window.__searchClipboardAck={matches};")).unwrap();
                });return;
            }
            if let Some(fixture)=download_page.as_ref() && stage=="download-page-fixture" {
                let handle=handle.clone();let fixture=fixture.clone();
                tauri::async_runtime::spawn(async move {
                    let result=fixture.control(&handle.state::<MusicClient>(),&value).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__downloadPageAck={ack};")).unwrap();
                });return;
            }
            if search_fixture::enabled() && stage=="search-fixture" {
                search_fixture::control(&value);
                handle.get_webview_window("main").unwrap().eval("window.__searchFixtureAck=true;").unwrap();return;
            }
            if let Some(fixture)=transfer.as_ref() && stage=="cover-transfer-fixture" {
                let handle=handle.clone();let fixture=fixture.clone();
                tauri::async_runtime::spawn(async move {
                    let result=fixture.control(&handle.state::<MusicClient>(),&value).await;
                    let ack=match result {Ok(())=>json!({"ok":true,"report":fixture.report()}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__coverTransferAck={ack};")).unwrap();
                });return;
            }
            if playlist_cover_layout_fixture::enabled() && stage=="cover-fixture" {
                let handle=handle.clone();let root=cover_root.clone();let store=proof_store.clone();
                tauri::async_runtime::spawn(async move {
                    let client=handle.state::<MusicClient>();
                    let result=playlist_cover_layout_fixture::control(&client,&store,&root,&value).await;
                    let ack=match result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"message":error.to_string()})};
                    handle.get_webview_window("main").unwrap().eval(format!("window.__coverFixtureAck={ack};")).unwrap();
                });
                return;
            }
            if playlist_editor_fixture::enabled() && stage=="editor-fixture" {
                    playlist_editor_fixture::control(&value);
                    handle.get_webview_window("main").unwrap().eval("window.__editorFixtureAck=true;").unwrap();
                    return;
            }
            if playlist_collect_fixture::enabled() && stage=="collect-fixture" {
                playlist_collect_fixture::control(&value);
                let report = playlist_collect_fixture::report();
                handle.get_webview_window("main").unwrap().eval(format!("window.__collectFixtureAck={report};")).unwrap();
                return;
            }
            if library_fixture::enabled() && stage=="library-fixture" {
                library_fixture::control(&value);
                let report = library_fixture::report();
                handle.get_webview_window("main").unwrap().eval(format!("window.__libraryFixtureAck={report};")).unwrap();
                return;
            }
            if playlist_layout_fixture::enabled() && stage=="playlist-fixture-release" {
                playlist_layout_fixture::release_failure();
                return;
            }
            if shortcut_proof && stage=="shortcut-fixture" {
                let epoch=handle.state::<MusicClient>().account().unwrap().epoch;
                media_shortcuts::command(&handle,&proof_store,&epoch,&value).unwrap();
                handle.get_webview_window("main").unwrap().eval(format!("window.__shortcutFixtureAck={};",serde_json::to_string(&value["kind"]).unwrap())).unwrap();
                return;
            }
            if playback_quality_fixture::enabled() && stage=="quality-fixture" {
                playback_quality_fixture::control(&proof_store,&value);
                let report=playback_quality_fixture::report();
                handle.get_webview_window("main").unwrap().eval(format!("window.__qualityFixtureAck={report};")).unwrap();
                return;
            }
            if mode_fixture::enabled() && stage=="mode-fixture" {
                let kind=value["kind"].as_str().unwrap();mode_fixture::control(&proof_store,kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__modeFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();
                return;
            }
            if volume_fixture::enabled() && stage=="volume-fixture" {
                let kind=value["kind"].as_str().unwrap();
                volume_fixture::apply(&proof_store,kind);
                handle.get_webview_window("main").unwrap().eval(format!("window.__volumeFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();
                return;
            }
            if media_proof && stage=="media-fixture" {
                let kind=value["kind"].as_str().unwrap();
                match kind {
                    "reject" | "allow" => {
                        let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                        sql.execute_batch(if kind=="reject" {"CREATE TRIGGER reject_media_ui BEFORE INSERT ON state WHEN NEW.kind='system-media-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic setting write failure'); END;"} else {"DROP TRIGGER reject_media_ui;"}).unwrap();
                    }
                    "damage"=>proof_store.put("local-device","system-media-preferences-v1","current",&json!({"enabled":"invalid"})).unwrap(),
                    "raw_damage"=>{
                        let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                        assert_eq!(sql.execute("UPDATE state SET value=? WHERE owner='local-device' AND kind='system-media-preferences-v1' AND key='current'",["{broken"]).unwrap(),1);
                    }
                    _=>panic!("unknown isolated media fixture action"),
                }
                handle.get_webview_window("main").unwrap().eval(format!("window.__mediaFixtureAck={};",serde_json::to_string(kind).unwrap())).unwrap();
                return;
            }
            if let Some(fixture)=recovery.as_ref() {
                use std::sync::atomic::Ordering;
                if stage=="like-fixture" {
                    if let Some(healthy)=value["healthy"].as_bool(){fixture.healthy.store(healthy,Ordering::SeqCst);}
                    if let Some(remote)=value["remote"].as_bool(){fixture.remote(value["id"].as_str().unwrap(),remote);}
                    if value["failRead"]==true{fixture.fail_read.store(true,Ordering::SeqCst);}
                    return;
                }
            }
            if stage=="compact-zoom" && download_page_fixture::enabled() {
                let scale=match value["enabled"].as_bool(){Some(true)=>1.25,Some(false)=>1.0,None=>return};
                handle.get_webview_window("main").unwrap().set_zoom(scale).unwrap();return;
            }
            if stage=="focus-sample" && download_page_fixture::enabled() {
                handle.get_webview_window("main").unwrap().set_focus().unwrap();return;
            }
            if stage=="resize" {
                handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(value["width"].as_f64().unwrap(),value["height"].as_f64().unwrap())).unwrap();return;
            }
            if !(["complete","failed"].contains(&stage) || stage.starts_with("ux_") && stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')){return;}
            let stage=stage.to_owned();
            if playback_quality_fixture::enabled() { value["syntheticQuality"] = playback_quality_fixture::report(); }
            if mode_fixture::enabled() { value["syntheticMode"] = mode_fixture::report(); }
            if library_fixture::enabled() { value["syntheticLibrary"] = library_fixture::report(); }
            if playlist_collect_fixture::enabled() { value["syntheticCollection"] = playlist_collect_fixture::report(); }
            if let Some(fixture)=transfer.as_ref() {value["syntheticCoverTransfer"]=fixture.report();}
            if playlist_editor_fixture::enabled() {value["syntheticMetadataWrites"]=json!(playlist_editor_fixture::writes());}
            if let Some(fixture)=recovery.as_ref() {
                use std::sync::atomic::Ordering;
                value["syntheticLikeWrites"]=json!(fixture.writes.load(Ordering::SeqCst));
                value["syntheticLikeReads"]=json!(fixture.reads.load(Ordering::SeqCst));
            }
            if motion_proof {
                value["audioFactoryElapsedMs"] = json!(factory_elapsed.load(std::sync::atomic::Ordering::Acquire));
                value["audioFactoryDelayRequestedMs"] = json!(startup_delay);
                value["audioFactoryCount"] = json!(factory_count.load(std::sync::atomic::Ordering::Acquire));
                if media_proof {
                    value["systemMediaStatus"]=serde_json::to_value(cumulus_desktop::system_media::settings::system_media_status(handle.clone())).unwrap();
                    value["systemMediaPreferences"]=serde_json::to_value(handle.state::<MusicClient>().system_media_preferences_native().unwrap()).unwrap();
                }
                if shortcut_proof {media_shortcuts::observe(&handle,&mut value);}
                let stage = stage.to_owned(); let handle=handle.clone(); let output=output.clone();
                tauri::async_runtime::spawn(async move {
                    let client=handle.state::<MusicClient>(); let epoch=client.account().unwrap().epoch;
                    if let Ok(playback)=client.subscribe_playback(&epoch).await && let Ok(snapshot)=playback.current() {
                        value["nativePlayer"]=serde_json::to_value(snapshot).unwrap();
                        if std::env::var_os("CUMULUS_PLAYLIST_QUEUE_UI_PROOF").is_some() || std::env::var_os("CUMULUS_QUEUE_UI_PROOF").is_some() || download_page_fixture::enabled() {
                            value["nativeQueue"] = serde_json::to_value(client.player_queue(&epoch).await.unwrap()).unwrap();
                        }
                    } else { value["nativePlayerUnavailable"]=json!(true); }
                    fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if !capture_ack::acknowledge(&handle,&output,&stage,value["captureHandshake"]==true).await {return;}
                    if (std::env::var_os("CUMULUS_QUEUE_SELECTION_UI_PROOF").is_some() || intelligence_ui_fixture::enabled() || intelligence_start_fixture::enabled()) && stage.starts_with("ux_") {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__queueSelectionRecorded={};",serde_json::json!(stage))).unwrap();
                    }
                    if ["complete","failed"].contains(&stage.as_str()) {
                        let _=client.player_action(&epoch,cumulus_core::client::dto::PlayerAction::Stop).await;
                        handle.exit(if stage=="complete" {0} else {1});
                    }
                });
                return;
            }
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1") && stage.starts_with("ux_") {
                let handle=handle.clone();let output=output.clone();
                tauri::async_runtime::spawn(async move {capture_ack::acknowledge(&handle,&output,&stage,value["captureHandshake"]==true).await;});
                return;
            }
            if ["complete","failed"].contains(&stage.as_str()) {handle.exit(if stage=="complete" {0} else {1});}
        });
        // Absolute config dataDirectory values are ignored by Tauri's safe-path
        // resolver. The builder API owns this isolated test directory explicitly.
        let mut builder=tauri::WebviewWindowBuilder::from_config(app, &window_config)?.data_directory(root.join("webview"));
        if motion_proof { builder=builder.initialization_script(include_str!("motion_environment.js")); }
        builder.build()?;
        if media_proof {cumulus_desktop::system_media::install(app.handle());}
        if shortcut_proof {cumulus_desktop::shortcuts::install(app.handle());}
        Ok(())
    }).on_page_load(|view, payload| {
        if payload.event()==PageLoadEvent::Finished && view.label()=="main" {
            let script=std::env::var_os("CUMULUS_UX_SCRIPT").map(|p|fs::read_to_string(p).unwrap());
            let default_script = if std::env::var_os("CUMULUS_LIKE_RECOVERY_PROOF").is_some() { include_str!("like_recovery_ui_probe.js") } else if std::env::var_os("CUMULUS_MOTION_PROOF").is_some() { include_str!("motion_ui_probe.js") } else { include_str!("ux_ui_probe.js") };
            let source=script.as_deref().unwrap_or(default_script);
            if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1") {view.eval(capture_ack::script(source)).unwrap();}
            else {view.eval(source).unwrap();}
        }
    }).run(context).expect("UX probe");
}
