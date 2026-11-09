//! Two real WebViews over one production MusicClient and synthetic PCM/null AO.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/cloud_file_fixture.rs"]
mod cloud_file_fixture;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[path = "support/mini_fixture.rs"]
mod mini_fixture;
#[path = "support/mini_pin_fixture.rs"]
mod mini_pin_fixture;
use mini_fixture::intelligence_start_fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
#[path = "support/shortcut_foreground.rs"]
mod shortcut_foreground;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tauri::{
    Listener, Manager,
    utils::assets::{AssetKey, AssetsIter, CspHash},
    webview::PageLoadEvent,
};
struct FaultAssets {
    files: BTreeMap<String, Vec<u8>>,
    block: Arc<AtomicBool>,
    lyric_block: Arc<AtomicBool>,
}
impl tauri::Assets<tauri::Wry> for FaultAssets {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        if self.lyric_block.load(Ordering::Acquire)
            && key.as_ref().contains("DesktopLyricApp-")
            && key.as_ref().ends_with(".js")
        {
            return Some(Cow::Borrowed(b"export default {};"));
        }
        if self.block.load(Ordering::Acquire)
            && key.as_ref().contains("MiniApp-")
            && key.as_ref().ends_with(".js")
        {
            return Some(Cow::Borrowed(b"export default {};"));
        }
        self.files
            .get(key.as_ref())
            .map(|v| Cow::Borrowed(v.as_slice()))
    }
    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(
            self.files
                .iter()
                .map(|(k, v)| (Cow::Borrowed(k.as_str()), Cow::Borrowed(v.as_slice()))),
        )
    }
    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}
#[cfg(target_os = "linux")]
fn startup_locale(category: libc::c_int) -> String {
    // SAFETY: query only, in Tauri startup after GTK and before player
    // workers. Copy the process-owned string before querying another category.
    unsafe {
        let value = libc::setlocale(category, std::ptr::null());
        assert!(!value.is_null());
        std::ffi::CStr::from_ptr(value)
            .to_string_lossy()
            .into_owned()
    }
}

fn main() {
    // SAFETY: before creating any runtime threads or GTK widgets.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    fs::create_dir_all(&root).unwrap();
    let output = root.join("first");
    fs::create_dir(&output).expect("new proof directory");
    let block = Arc::new(AtomicBool::new(false));
    let lyric_block = Arc::new(AtomicBool::new(false));
    let mut context = assets::context();
    let config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let files = context
        .assets()
        .iter()
        .map(|(k, _)| {
            let bytes = context
                .assets()
                .get(&AssetKey::from(k.as_ref()))
                .expect("embedded asset")
                .into_owned();
            (k.into_owned(), bytes)
        })
        .collect();
    context.set_assets(Box::new(FaultAssets {
        files,
        block: block.clone(),
        lyric_block: lyric_block.clone(),
    }));
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        #[cfg(target_os = "linux")]
        fs::write(output.join("locale.json"),serde_json::to_vec(&json!({
            "numeric":startup_locale(libc::LC_NUMERIC),
            "messages":startup_locale(libc::LC_MESSAGES),
            "characters":startup_locale(libc::LC_CTYPE)
        }))?)?;
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let proof_store=registry.store();
        let api=Arc::new(mini_fixture::Library::default());
        let session=Arc::new(AccountSession::new(api.clone(),Some(registry.store())));
        let count=Arc::new(AtomicUsize::new(0));
        let factory_count=count.clone(); let wave=relay_fixture::wave(&root)?;
        let local_media_directory=std::env::var_os("CUMULUS_LOCAL_RESOURCE_UI_PROOF").map(|_|root.join("menu-originals"));
        let client=MusicClient::with_session(session.clone(),registry).with_audio_factory(move || {
            factory_count.fetch_add(1,Ordering::AcqRel);
            let audio=relay_fixture::NativeAudio::open(wave.clone())?;
            Ok(Box::new(match &local_media_directory {
                Some(directory)=>audio.allow_download_directory(directory.clone()),
                None=>audio,
            }))
        });
        let client = if mini_fixture::cloud_ui::batch_enabled() {
            client.with_download_backend(Arc::new(mini_fixture::cloud_ui::media::Backend))
        } else {client};
        let client=if mini_fixture::upload_ui::enabled() { api.upload_ui.initialize(&root)?; client.with_cloud_adapters(api.upload_ui.adapters()) } else { client };
        app.manage(client);
        app.add_capability(r#"{"identifier":"mini-proof","windows":["main","mini-player-*"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        let cloud_removal_lock=Arc::new(std::sync::Mutex::new(None::<cumulus_core::storage::OperationLock>));
        app.listen("mini-proof", move |event| {
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let kind=value["kind"].as_str().unwrap_or("");
            let id=value["id"].clone();
            let ack=|value:Value| { handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,value)).unwrap(); };
            match kind {
                "upload-state"=>ack(api.upload_ui.report()),
                "upload-config"=>{api.upload_ui.configure(&value);ack(json!({"ok":true}));}
                "upload-task-seed"=>{
                    assert!(mini_fixture::upload_ui::enabled());
                    let api=api.clone();let session=session.clone();let root=root.clone();let handle=handle.clone();let count=value["count"].as_u64().unwrap() as usize;
                    tauri::async_runtime::spawn(async move {
                        let reply=match api.upload_ui.seed(&root,session,count).await{Ok(ids)=>json!({"ids":ids}),Err(error)=>json!({"error":cumulus_core::client::ClientError::from(error).message})};
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,reply)).unwrap();
                    });
                }
                "upload-directory"=>{
                    let handle=handle.clone();let task_id=value["taskId"].as_str().unwrap().to_owned();let epoch=value["epoch"].as_str().unwrap().to_owned();let root=root.clone();
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();
                        let reply=match client.cloud_upload_source_reveal(&epoch,&task_id).await{Ok(reveal)=>{let path=reveal.path();assert!(path.is_file()&&path.canonicalize().unwrap().starts_with(root.join("upload-inputs").canonicalize().unwrap()));reveal.check_current().unwrap();json!({"verified":true,"leafVerified":true,"osLauncherInjected":true})},Err(error)=>json!({"error":cumulus_core::client::ClientError::from(error).message})};
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,reply)).unwrap();
                    });
                }
                "upload-select"=>{
                    assert!(mini_fixture::upload_ui::enabled());
                    let paths=api.upload_ui.selection(&root,value["selection"].as_str().unwrap());
                    let audio=matches!(value["pickKind"].as_str(),Some("audio"|"folder"));
                    let handle=handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
                        let paths=if audio {client.select_cloud_sources(&epoch,paths).await.unwrap()}else{paths.into_iter().map(|p|p.to_string_lossy().into_owned()).collect()};
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,json!({"ok":true,"paths":paths}))).unwrap();
                    });
                }
                "cloud-local-file"=>{
                    assert!(mini_fixture::cloud_ui::enabled());
                    let action=value["action"].as_str().unwrap().to_owned();let root=root.clone();let session=session.clone();let store=proof_store.clone();let handle=handle.clone();
                    let epoch=value["epoch"].as_str().unwrap_or("").to_owned();let revision=value["revision"].as_str().unwrap_or("").to_owned();
                    tauri::async_runtime::spawn(async move {
                        let outcome=async {
                            let path=root.join("cloud-local-source/mapped.flac");
                            if action=="verify" {
                                let directory=handle.state::<MusicClient>().cloud_directory_path(&epoch,"1001",&revision).await?;
                                assert_eq!(directory.canonicalize()?,path.parent().unwrap().canonicalize()?);
                                return Ok(json!({"verified":true,"osLauncherInjected":true}));
                            }
                            tauri::async_runtime::spawn_blocking(move || -> cumulus_core::Result<Value> {
                                let lib=cumulus_core::library::LocalLibrary::new(store.clone())?;
                                match action.as_str() {
                                    "map"=>{
                                        fs::create_dir_all(path.parent().unwrap())?;fs::write(&path,mini_fixture::cloud_ui::media::DATA)?;
                                        lib.scan(path.parent().unwrap())?;
                                        let item=lib.list(Default::default())?.into_iter().find(|item|item.path==path.canonicalize().unwrap()).unwrap();
                                        lib.map_online(&session,&item.id,&json!(900001))?;
                                    }
                                    "change"=>{let mut bytes=fs::read(&path)?;bytes.push(0);fs::write(&path,bytes)?;}
                                    "corrupt"|"repair"=>{let token=session.token()?;let uid=token.uid.as_deref().expect("synthetic signed in account");
                                        if action=="corrupt"{store.put(uid,"local_map_v1","damaged",&json!("synthetic invalid mapping"))?;}else{store.delete(uid,"local_map_v1","damaged")?;}
                                        assert_eq!(session.token()?,token);
                                    }
                                    _=>panic!("unknown cloud local proof action"),
                                }
                                Ok(json!({"ok":true,"actualFfprobe":true}))
                            }).await.map_err(|_|cumulus_core::Error::State("synthetic cloud local worker stopped"))?
                        }.await;
                        let reply=match outcome {Ok(value)=>value,Err(error)=>json!({"error":cumulus_core::client::ClientError::from(error).message})};
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,reply)).unwrap();
                    });
                }
                "cloud-config"=>{api.cloud_ui.configure(&value);ack(json!({"ok":true}));}
                "cloud-state"=>ack(api.cloud_ui.report()),
                "cloud-menu-seed"=>{
                    assert!(mini_fixture::cloud_ui::menu_enabled());
                    let handle=handle.clone();let directory=root.join("menu-originals");
                    let song_id=value["songId"].as_str().unwrap_or("1004").to_owned();assert!(["1004","1005","1006"].contains(&song_id.as_str()));
                    tauri::async_runtime::spawn(async move {
                        use cumulus_core::client::dto::*;
                        let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
                        let page=client.cloud_page(&epoch,CloudQuery{tab:CloudTab::Uploaded,keyword:String::new(),sort:CloudSort::Natural,descending:false,offset:0,limit:200},true).await.unwrap();
                        let completed=client.downloads(&epoch,DownloadQuery::default()).await.unwrap().completed_count;
                        client.download_command(&epoch,DownloadCommand::Configure{root:directory.to_string_lossy().into_owned(),path_type:2,file_type:1}).await.unwrap();
                        client.download_command(&epoch,DownloadCommand::StartCloud{song_ids:vec![song_id.clone()],revision:page.revision.clone().unwrap(),lyrics:false}).await.unwrap();
                        for _ in 0..200 {
                            let view=client.downloads(&epoch,DownloadQuery::default()).await.unwrap();
                            if view.completed_count>completed {break;}
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        }
                        let path=client.cloud_directory_path(&epoch,&song_id,&page.revision.unwrap()).await.unwrap();
                        assert!(path.canonicalize().unwrap().starts_with(directory.canonicalize().unwrap()));
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={{ok:true,verifiedOriginal:true}};",id)).unwrap();
                    });
                }
                "cloud-removal-file"=>{
                    assert!(mini_fixture::cloud_ui::menu_enabled());
                    let action=value["action"].as_str().unwrap().to_owned();let song_id=value["songId"].as_str().unwrap().to_owned();assert!(["1004","1005","1006"].contains(&song_id.as_str()));
                    let session=session.clone();let root=root.clone();let store=proof_store.clone();let handle=handle.clone();let held=cloud_removal_lock.clone();
                    tauri::async_runtime::spawn(async move {
                        let result=tauri::async_runtime::spawn_blocking(move || -> cumulus_core::Result<Value> {
                            use cumulus_core::download::service::DownloadService;
                            let tasks=DownloadService::new(session)?.list()?;
                            let recorded=root.join(format!("removal-path-{song_id}.json"));
                            let target=tasks.iter().find(|t|t.song_id==song_id).and_then(|t|t.target.as_deref());
                            let path=cloud_file_fixture::original_path(&root.join("menu-originals"),&recorded,target)?;
                            match action.as_str(){
                                "observe"=>{},
                                "index"=>{cumulus_core::library::LocalLibrary::new(store.clone())?.scan(root.join("menu-originals"))?;},
                                "hold-index"=>{*held.lock().unwrap()=Some(cumulus_core::storage::OperationLock::acquire(store.directory(),"local","library-index")?);},
                                "release-index"=>{held.lock().unwrap().take();},
                                "change"=>{let mut bytes=fs::read(&path)?;bytes.push(0);fs::write(&path,bytes)?;},
                                "cue"=>{
                                    let cue=path.with_extension("cue");fs::write(&cue,format!("FILE \"{}\" WAVE\n TRACK 01 AUDIO\n INDEX 01 00:00:00\n",path.file_name().unwrap().to_str().unwrap()))?;
                                    cumulus_core::library::LocalLibrary::new(store)?.import_cue(cue)?;
                                },
                                _=>panic!("unknown synthetic file mutation"),
                            }
                            Ok(json!({"exists":path.is_file(),"downloadPresent":tasks.iter().any(|t|t.song_id==song_id)}))
                        }).await;
                        let reply=match result{Ok(Ok(v))=>v,Ok(Err(e))=>json!({"error":e.to_string()}),Err(_)=>json!({"error":"synthetic file inspection worker stopped"})};
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,reply)).unwrap();
                    });
                }
                "cloud-download-root"=>{
                    assert!(mini_fixture::cloud_ui::batch_enabled());
                    let handle=handle.clone();let directory=root.join("batch-downloads");
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();let epoch=client.account().unwrap().epoch;
                        client.download_command(&epoch,cumulus_core::client::dto::DownloadCommand::Configure{root:directory.to_string_lossy().into_owned(),path_type:2,file_type:1}).await.unwrap();
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{}]={};",id,json!({"ok":true,"root":directory}))).unwrap();
                    });
                }
                "cloud-cache-break"=>{
                    let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                    assert_eq!(sql.execute("UPDATE state SET value=? WHERE kind='cloud_library_v1' AND key='snapshot'",["{"]).unwrap(),1);
                    ack(json!({"ok":true}));
                }
                "report-config"=>{api.lyric_reports.configure(&value);ack(json!({"ok":true}));}
                "report-state"=>ack(api.lyric_reports.report()),
                "report-resize"=>{let scale=value["scale"].as_f64().unwrap();handle.get_webview_window("main").unwrap().set_size(tauri::PhysicalSize::new((value["width"].as_f64().unwrap()*scale).round() as u32,(value["height"].as_f64().unwrap()*scale).round() as u32)).unwrap();ack(json!({"ok":true}));}
                "report-storage"=>{
                    let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                    if value["fail"]==true{sql.execute_batch("CREATE TRIGGER reject_lyric_report BEFORE INSERT ON state WHEN NEW.kind='lyric-reports-v1' BEGIN SELECT RAISE(ABORT,'synthetic lyric report storage failure'); END;").unwrap();}
                    else{sql.execute_batch("DROP TRIGGER IF EXISTS reject_lyric_report;").unwrap();}
                    ack(json!({"ok":true}));
                }
                "lyric-assets" => {lyric_block.store(value["blocked"]==true,Ordering::Release);ack(json!({"ok":true}));}
                "lyric-storage" => {
                    let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                    if value["fail"]==true {sql.execute_batch("CREATE TRIGGER reject_mini_lyric_pref BEFORE INSERT ON state WHEN NEW.owner='local-device' AND NEW.kind='desktop-lyric-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic mini lyric preference failure'); END;").unwrap();}
                    if value["fail"]==false {sql.execute_batch("DROP TRIGGER IF EXISTS reject_mini_lyric_pref;").unwrap();}
                    if value["corrupt"]==true {proof_store.put("local-device","desktop-lyric-preferences-v1","current",&json!("synthetic damaged desktop lyric settings")).unwrap();}
                    ack(json!({"ok":true}));
                }
                "exit-armed" => {
                    fs::write(output.join("exit-armed.json"),serde_json::to_vec_pretty(&value).unwrap()).unwrap();ack(json!({"ok":true}));
                }
                "pin-native" => {
                    let mini=handle.webview_windows().into_iter().find(|(label,_)|label.starts_with("mini-player-")).expect("mini window").1;
                    if let Some(pinned)=value["set"].as_bool() {
                        mini.set_always_on_top(pinned).unwrap();
                        fs::write(output.join("pin-native-request.json"),serde_json::to_vec(&value).unwrap()).unwrap();
                    }
                    let client=handle.state::<MusicClient>();
                    let epoch=client.account().unwrap().epoch;
                    if let Some(pinned)=value["save"].as_bool() {
                        let revision=client.mini_preferences(&epoch).unwrap().value.revision.unwrap();
                        client.mini_preferences_command(&epoch,cumulus_core::client::dto::MiniPreferencesCommand::Change{expected_revision:revision,always_on_top:pinned}).unwrap();
                    }
                    if value["corrupt"]==true { proof_store.put("local-device","mini-preferences-v1","current",&json!("synthetic damaged pin preference")).unwrap(); }
                    let pin=json!({"applied":mini_pin_fixture::applied(&mini),"tauriCached":mini.is_always_on_top().unwrap(),"preferences":client.mini_preferences(&epoch).unwrap().value,"requested":value.get("set")});
                    #[cfg(target_os="linux")]
                    let pin={
                        use gtk::prelude::*;
                        let mut pin=pin;
                        pin["gdkAbove"]=json!(mini.gtk_window().unwrap().window().map(|w|w.state().contains(gtk::gdk::WindowState::ABOVE)));
                        pin
                    };
                    ack(pin);
                }
                "pin-storage" => {
                    let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                    sql.execute_batch(if value["reject"]==true {
                        "CREATE TRIGGER reject_mini_pin_insert BEFORE INSERT ON state WHEN NEW.owner='local-device' AND NEW.kind='mini-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic mini pin commit failure'); END;
                         CREATE TRIGGER reject_mini_pin_update BEFORE UPDATE ON state WHEN NEW.owner='local-device' AND NEW.kind='mini-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic mini pin commit failure'); END;"
                    } else { "DROP TRIGGER reject_mini_pin_insert; DROP TRIGGER reject_mini_pin_update;" }).unwrap();ack(json!({"ok":true}));
                }
                "fm-feedback" => {
                    let owner=session.token().unwrap().uid.unwrap();
                    api.fm.control(&proof_store,&owner,value["action"].as_str().unwrap());
                    ack(api.fm.report());
                }
                "eval" => {
                    let mini=handle.webview_windows().into_iter().find(|(label,_)|label.starts_with("mini-player-")).expect("mini window").1;
                    let source=value["source"].as_str().unwrap();
                    mini.eval(format!(r#"(async()=>{{try{{const value=await (async()=>{{{source}}})();await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'mini-proof',payload:{{kind:'result',id:{id},value}}}});}}catch(e){{await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'mini-proof',payload:{{kind:'result',id:{id},error:String(e?.message||e),code:e?.code}}}});}}}})();"#)).unwrap();
                }
                "result" => ack(value.clone()),
                "intelligence" => { intelligence_start_fixture::control(value["action"].as_str().unwrap());ack(intelligence_start_fixture::report()); }
                "storage" => {
                    let sql=rusqlite::Connection::open(proof_store.directory().join("core.sqlite3")).unwrap();
                    sql.execute_batch(if value["reject"]==true {
                        "CREATE TRIGGER reject_mini_mode BEFORE UPDATE ON state WHEN NEW.owner='2' AND NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic mini commit failure'); END;"
                    } else { "DROP TRIGGER reject_mini_mode;" }).unwrap();ack(json!({"ok":true}));
                }
                "key" => {
                    let id = id.as_u64().expect("key sequence");
                    match shortcut_foreground::grant() {
                        Ok(Some(grant)) => fs::write(output.join(format!("foreground-{id}.json")),serde_json::to_vec_pretty(&grant).unwrap()).unwrap(),
                        Ok(None) => (),
                        Err(error) => { ack(json!({"error":error}));return; }
                    }
                    let request = output.join(format!("key-{id}.json"));
                    let pending = request.with_extension("pending");
                    fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
                    fs::rename(pending, request).unwrap();
                    let reply = output.join(format!("key-{id}.done"));
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        for _ in 0..100 {
                            if reply.exists() {
                                handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{id}]={{ok:true}};")).unwrap();
                                return;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        handle.get_webview_window("main").unwrap().eval(format!("window.__miniAck[{id}]={{error:'independent keyboard did not reply'}};")).unwrap();
                    });
                }
                "block" => { block.store(value["enabled"]==true,Ordering::Release); ack(json!({"ok":true})); }
                "account" => { session.adopt(api.clone(),json!({"userId":99}),&session.token().unwrap()).unwrap(); ack(json!({"ok":true})); }
                "show" => { cumulus_desktop::lifecycle::show(&handle); ack(json!({"ok":true})); }
                "windows" => ack(json!({"main":handle.get_webview_window("main").unwrap().is_visible().unwrap(),"mini":handle.webview_windows().keys().filter(|l|l.starts_with("mini-player-")).collect::<Vec<_>>(),"lyrics":handle.webview_windows().keys().filter(|l|l.starts_with("desktop-lyrics-")).collect::<Vec<_>>()})),
                "snapshot" | "complete" | "failed" => {
                    let mut value=value.clone(); let output=output.clone(); let handle=handle.clone();
                    value["factoryCount"]=json!(count.load(Ordering::Acquire));
                    value["mainVisible"]=json!(handle.get_webview_window("main").unwrap().is_visible().unwrap());
                    value["miniWindows"]=json!(handle.webview_windows().keys().filter(|l|l.starts_with("mini-player-")).collect::<Vec<_>>());
                    value["lyricWindows"]=json!(handle.webview_windows().keys().filter(|l|l.starts_with("desktop-lyrics-")).collect::<Vec<_>>());
                    value["likeWritesObserved"]=json!(api.likes.lock().unwrap().clone());
                    if mini_fixture::lyric_reports::enabled(){value["syntheticLyricReports"]=api.lyric_reports.report();}
                    if mini_fixture::cloud_ui::enabled(){value["syntheticCloud"]=api.cloud_ui.report();}
                    if mini_fixture::upload_ui::enabled(){value["syntheticUpload"]=api.upload_ui.report();}
                    value["syntheticRoom"]=api.room.report();
                    if mini_fixture::fm::enabled() { value["syntheticFm"]=api.fm.report(); }
                    if intelligence_start_fixture::enabled() { value["syntheticIntelligence"]=intelligence_start_fixture::report(); }
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();
                        let epoch=client.account().unwrap().epoch;
                        value["nativePlayer"]=serde_json::to_value(client.subscribe_playback(&epoch).await.unwrap().current().unwrap()).unwrap();
                        value["nativeMini"]=serde_json::to_value(client.mini_snapshot().unwrap().view).unwrap();
                        if mini_fixture::fm::enabled() {
                            let source=serde_json::from_value(value.get("fmSource").cloned().unwrap_or_else(||json!({"kind":"personal"}))).unwrap();
                            match client.fm_view(&epoch,source,false).await {
                                Ok(view)=>value["nativeFm"]=serde_json::to_value(view.value).unwrap(),
                                Err(error)=>value["nativeFmFailure"]=serde_json::to_value(cumulus_core::client::ClientError::from(error)).unwrap(),
                            }
                        }
                        let stage=value["stage"].as_str().unwrap_or("failed");
                        assert!(stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'));
                        fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                        if stage.starts_with("ux_") {
                            if std::env::var_os("CUMULUS_PROOF_CAPTURE_ACK").is_some() {
                                let ack=output.join(format!("{stage}.captured"));
                                for _ in 0..250 { if ack.exists() { break; } tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
                                assert!(ack.exists(),"mini screenshot acknowledgement missing");
                            }
                            handle.get_webview_window("main").unwrap().eval(format!("window.__miniRecorded={};",json!(stage))).unwrap();
                        }
                        if stage=="complete" || stage=="failed" { handle.exit(if stage=="complete" {0}else{1}); }
                    });
                }
                _ => panic!("unknown synthetic mini fixture command"),
            }
        });
        tauri::WebviewWindowBuilder::from_config(app,&config)?.build()?;
        cumulus_desktop::lifecycle::install(app.handle())?;
        cumulus_desktop::shortcuts::install(app.handle());
        Ok(())
    }).on_page_load(|view,payload| {
        if view.label()=="main" && payload.event()==PageLoadEvent::Finished {
            let source=if let Some(path)=std::env::var_os("CUMULUS_MINI_SCRIPT") { fs::read_to_string(path).unwrap()
            } else if std::env::var_os("CUMULUS_MINI_SHORTCUT_PROOF").is_some() {
                include_str!("mini_shortcuts_ui_probe.js").into()
            } else {
                include_str!("mini_ui_probe.js").into()
            };
            let source=if std::env::var_os("CUMULUS_MINI_PIN_UNMANAGED_PROOF").is_some() { format!("window.__pinUnmanagedProof=true;\n{source}") } else {source};
            let source=if std::env::var_os("CUMULUS_MINI_MENU_EXIT_PROOF").is_some() { format!("window.__miniMenuExitProof=true;\n{source}") } else {source};
            view.eval(source).unwrap();
        }
    }).run(context).expect("mini WebView proof");
}
