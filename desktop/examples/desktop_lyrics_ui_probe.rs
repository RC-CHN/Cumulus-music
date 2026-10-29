//! Main/settings and a transparent lyric WebView over one real native PCM/null engine.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/desktop_lyric_fixture.rs"]
mod desktop_lyric_fixture;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[path = "support/mini_fixture.rs"]
mod mini_fixture;
#[allow(dead_code)]
#[path = "support/relay_fixture.rs"]
mod relay_fixture;
#[allow(dead_code)]
#[path = "support/settings_fixture.rs"]
mod settings_fixture;
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
}
impl tauri::Assets<tauri::Wry> for FaultAssets {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        if self.block.load(Ordering::Acquire)
            && key.as_ref().contains("DesktopLyricApp-")
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
fn main() {
    // SAFETY: before creating any runtime threads or GTK widgets.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    fs::create_dir_all(&root).unwrap();
    let reopened = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let output = root.join(if reopened { "reopen" } else { "first" });
    fs::create_dir(&output).expect("new proof directory");
    let block = Arc::new(AtomicBool::new(false));
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
    }));
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let api=Arc::new(desktop_lyric_fixture::Library::default());
        let session=Arc::new(AccountSession::new(api.clone(),Some(registry.store())));
        let count=Arc::new(AtomicUsize::new(0));
        let factory_count=count.clone(); let wave=relay_fixture::wave(&root)?;
        let client=MusicClient::with_session(session.clone(),registry).with_audio_factory(move || {
            factory_count.fetch_add(1,Ordering::AcqRel);
            Ok(Box::new(relay_fixture::NativeAudio::open(wave.clone())?))
        });
        app.manage(client);
        app.add_capability(r#"{"identifier":"desktop-lyric-proof","windows":["main","desktop-lyrics-*"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("desktop-lyric-proof", move |event| {
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let kind=value["kind"].as_str().unwrap_or("");
            let id=value["id"].clone();
            let ack=|value:Value| { handle.get_webview_window("main").unwrap().eval(format!("window.__lyricAck[{}]={};",id,value)).unwrap(); };
            match kind {
                "eval" => {
                    let mini=handle.webview_windows().into_iter().find(|(label,_)|label.starts_with("desktop-lyrics-")).expect("mini window").1;
                    let source=value["source"].as_str().unwrap();
                    mini.eval(format!(r#"(async()=>{{try{{const value=await (async()=>{{{source}}})();await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'desktop-lyric-proof',payload:{{kind:'result',id:{id},value}}}});}}catch(e){{await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'desktop-lyric-proof',payload:{{kind:'result',id:{id},error:String(e?.message||e)}}}});}}}})();"#)).unwrap();
                }
                "result" => ack(value.clone()),
                "key" => {
                    let id = id.as_u64().expect("key sequence");
                    let request = output.join(format!("key-{id}.json"));
                    let pending = request.with_extension("pending");
                    fs::write(&pending, serde_json::to_vec(&value).unwrap()).unwrap();
                    fs::rename(pending, request).unwrap();
                    let reply = output.join(format!("key-{id}.done"));
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        for _ in 0..100 {
                            if reply.exists() {
                                handle.get_webview_window("main").unwrap().eval(format!("window.__lyricAck[{id}]={{ok:true}};")).unwrap();
                                return;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        handle.get_webview_window("main").unwrap().eval(format!("window.__lyricAck[{id}]={{error:'independent keyboard did not reply'}};")).unwrap();
                    });
                }
                "hold" => { api.held.store(value["enabled"]==true, Ordering::Release); if value["enabled"]!=true {api.resume.notify_waiters();} ack(json!({"ok":true})); }
                "offline" => { api.offline.store(value["enabled"]==true, Ordering::Release); ack(json!({"ok":true})); }
                "stats" => ack(json!({"requests":api.lyrics.load(Ordering::Acquire),"pending":api.pending.load(Ordering::Acquire),"factoryCount":count.load(Ordering::Acquire)})),
                "storage" => {
                    let sql=rusqlite::Connection::open(root.join("state/core.sqlite3")).unwrap();
                    if value["fail"]==true {
                        sql.execute_batch("CREATE TRIGGER reject_lyric_prefs BEFORE INSERT ON state WHEN NEW.kind='desktop-lyric-preferences-v1' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
                    } else { sql.execute_batch("DROP TRIGGER IF EXISTS reject_lyric_prefs;").unwrap(); }
                    if value["corrupt"]==true { sql.execute("UPDATE state SET value='{' WHERE owner='local-device' AND kind='desktop-lyric-preferences-v1' AND key='current'",[]).unwrap(); }
                    ack(json!({"ok":true}));
                }
                "block" => { block.store(value["enabled"]==true,Ordering::Release); ack(json!({"ok":true})); }
                "account" => { session.adopt(api.clone(),json!({"userId":99}),&session.token().unwrap()).unwrap(); ack(json!({"ok":true})); }
                "show" => { cumulus_desktop::lifecycle::show(&handle); ack(json!({"ok":true})); }
                "focus_lyrics" => {
                    handle.webview_windows().into_iter().find(|(label,_)|label.starts_with("desktop-lyrics-")).unwrap().1.set_focus().unwrap();
                    ack(json!({"ok":true}));
                }
                "resize_main" => {
                    let width=value["width"].as_u64().expect("synthetic width");
                    let height=value["height"].as_u64().expect("synthetic height");
                    assert!((760..=1600).contains(&width) && (540..=1000).contains(&height));
                    handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(width as u32,height as u32)).unwrap();
                    ack(json!({"ok":true}));
                }
                "windows" => ack(json!({"main":handle.get_webview_window("main").unwrap().is_visible().unwrap(),"mini":handle.webview_windows().keys().filter(|l|l.starts_with("desktop-lyrics-")).collect::<Vec<_>>()})),
                "snapshot" | "complete" | "failed" => {
                    let mut value=value.clone(); let output=output.clone(); let handle=handle.clone();
                    value["factoryCount"]=json!(count.load(Ordering::Acquire));
                    value["mainVisible"]=json!(handle.get_webview_window("main").unwrap().is_visible().unwrap());
                    value["lyricWindows"]=json!(handle.webview_windows().keys().filter(|l|l.starts_with("desktop-lyrics-")).collect::<Vec<_>>());
                    value["lyricRequests"]=json!(api.lyrics.load(Ordering::Acquire));
                    value["pendingLyrics"]=json!(api.pending.load(Ordering::Acquire));
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();
                        let epoch=client.account().unwrap().epoch;
                        value["nativePlayer"]=serde_json::to_value(client.subscribe_playback(&epoch).await.unwrap().current().unwrap()).unwrap();
                        let stage=value["stage"].as_str().unwrap_or("failed");
                        assert!(stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'));
                        fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                        if stage=="complete" || stage=="failed" { handle.exit(if stage=="complete" {0}else{1}); }
                    });
                }
                _ => panic!("unknown synthetic mini fixture command"),
            }
        });
        tauri::WebviewWindowBuilder::from_config(app,&config)?.build()?;
        cumulus_desktop::lifecycle::install(app.handle())?;
        cumulus_desktop::shortcuts::install(app.handle());
        cumulus_desktop::desktop_lyrics::start(app.handle());
        Ok(())
    }).on_page_load(|view,payload| {
        if view.label()=="main" && payload.event()==PageLoadEvent::Finished {
            view.eval(format!("window.__lyricProofReopen={};", std::env::var_os("CUMULUS_PROOF_REOPEN").is_some())).unwrap();
            view.eval(include_str!("desktop_lyrics_ui_probe.js")).unwrap();
        }
    }).run(context).expect("mini WebView proof");
}
