//! Main/settings and a transparent lyric WebView over one real native PCM/null engine.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/capture_ack.rs"]
mod capture_ack;
#[path = "support/desktop_lyric_fixture.rs"]
mod desktop_lyric_fixture;
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;
#[path = "support/matched_local_lyric_fixture.rs"]
mod matched_local_lyric_fixture;
#[path = "support/mini_fixture.rs"]
mod mini_fixture;
#[path = "../../core/tests/support/native_lyric_wave.rs"]
mod native_lyric_wave;
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
    client::{
        MusicClient,
        dto::{LocalCommand, LocalQuery},
    },
    playback::engine::Cancellation,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
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
#[cfg(target_os = "linux")]
fn locale_name(category: libc::c_int) -> String {
    // SAFETY: the host's startup locale change has finished by the time
    // this also runs in a proof callback. Copy the C-owned value immediately.
    unsafe {
        let value = libc::setlocale(category, std::ptr::null());
        assert!(!value.is_null());
        std::ffi::CStr::from_ptr(value)
            .to_string_lossy()
            .into_owned()
    }
}

struct FontProof {
    mode: Mutex<&'static str>,
    reads: Mutex<BTreeMap<&'static str, usize>>,
}
impl FontProof {
    fn new(interface_phase: Option<&str>) -> Self {
        Self {
            mode: Mutex::new(match interface_phase {
                Some("reopen") => "interface-missing",
                Some(_) => "interface",
                None => "native",
            }),
            reads: Mutex::new(BTreeMap::new()),
        }
    }

    fn select(&self, mode: &str) {
        *self.mode.lock().unwrap() = match mode {
            "native" => "native",
            "failure" => "failure",
            "large" => "large",
            _ => panic!("unknown synthetic font catalog"),
        };
    }

    fn snapshot(&self) -> Value {
        let reads = self.reads.lock().unwrap();
        json!({"total":reads.values().sum::<usize>(),"sources":&*reads})
    }

    fn read(&self) -> Option<cumulus_core::Result<Vec<cumulus_desktop::FontCatalogProofFamily>>> {
        let mode = *self.mode.lock().unwrap();
        *self.reads.lock().unwrap().entry(mode).or_default() += 1;
        match mode {
            "native" => None,
            "failure" => Some(Err(cumulus_core::Error::State(
                "synthetic font enumeration failure",
            ))),
            "large" => Some(Ok((0..12_005)
                .map(|index| cumulus_desktop::FontCatalogProofFamily {
                    name: format!("Synthetic Font {index:05} with a deliberately long family name"),
                    aliases: vec![format!("合成字体 {index}")],
                })
                .collect())),
            "interface" | "interface-missing" => Some(Ok([
                ("华文琥珀", &[][..]),
                ("Proof Localized Family", &["合成黑体"][..]),
                ("楷体", &[][..]),
                ("Proof Latin Only", &[][..]),
                ("华文隶书", &[][..]),
                ("仿宋", &[][..]),
            ]
            .into_iter()
            .filter(|(name, _)| mode != "interface-missing" || *name != "楷体")
            .map(|(name, aliases)| cumulus_desktop::FontCatalogProofFamily {
                name: name.into(),
                aliases: aliases.iter().map(|alias| (*alias).into()).collect(),
            })
            .collect())),
            _ => unreachable!(),
        }
    }
}

fn main() {
    // SAFETY: before creating any runtime threads or GTK widgets.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("isolated proof root"));
    fs::create_dir_all(&root).unwrap();
    let reopened = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let fonts = std::env::var_os("CUMULUS_FONT_PROOF").is_some();
    let interface_font = match std::env::var("CUMULUS_INTERFACE_FONT_PROOF").as_deref() {
        Ok("first") => Some("first"),
        Ok("reopen") => Some("reopen"),
        Ok("reset") => Some("reset"),
        Err(std::env::VarError::NotPresent) => None,
        _ => panic!("invalid interface font proof phase"),
    };
    let font_fallback = std::env::var_os("CUMULUS_FONT_PROOF_FORCE_IPC_FALLBACK").is_some();
    assert!(!font_fallback || fonts);

    let embedded = std::env::var_os("CUMULUS_EMBEDDED_LYRIC_PROOF").is_some();
    let native_sidecar = std::env::var_os("CUMULUS_NATIVE_SIDECAR_LYRIC_PROOF").is_some();
    let credits = std::env::var_os("CUMULUS_LYRIC_CREDIT_PROOF").is_some();
    let matched = std::env::var_os("CUMULUS_MATCHED_LOCAL_LYRIC_PROOF").is_some();
    assert!(
        !matched
            || !(reopened
                || fonts
                || embedded
                || native_sidecar
                || credits
                || interface_font.is_some())
    );
    assert!(
        interface_font.is_none() || !(reopened || fonts || embedded || native_sidecar || credits),
        "interface font proof has its own phases"
    );
    #[cfg(target_os = "linux")]
    let original_messages = (native_sidecar || credits || matched || interface_font.is_some())
        .then(|| {
            // SAFETY: no GTK, runtime or worker has started in this process.
            assert!(!unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) }.is_null());
            assert!(!matches!(
                locale_name(libc::LC_NUMERIC).as_str(),
                "C" | "POSIX"
            ));
            locale_name(libc::LC_MESSAGES)
        });
    assert!(!(embedded && native_sidecar), "select one local lyric case");
    assert!(
        !(reopened && (embedded || native_sidecar)),
        "local lyric cases need a fresh profile"
    );
    assert!(
        !credits || (!embedded && !native_sidecar && !reopened),
        "credit proof requires fresh online fixture state"
    );
    let embedded_dir = embedded.then(|| {
        let directory = root.join("embedded-media");
        fs::create_dir(&directory).expect("new synthetic media directory");
        fs::write(
            directory.join("embedded.flac"),
            include_bytes!("../../core/tests/fixtures/tag_audio/seed.flac"),
        )
        .expect("write frozen synthetic tagged FLAC");
        fs::write(directory.join("embedded.lrc"), [0xff, 0xfe, 0x20])
            .expect("write synthetic truncated UTF-16 sidecar");
        directory
    });
    let native_path = native_sidecar.then(|| {
        let directory = root.join("native-media");
        let nested = directory.join("lyric/lrc");
        fs::create_dir_all(&nested).expect("new native lyric fixture folder");
        let name = "原生 合成.session.wav";
        let path = directory.join(name);
        fs::write(&path, native_lyric_wave::native_stereo_wave(10)).expect("write synthetic PCM");
        fs::write(
            nested.join(name).with_extension("lrc"),
            "[00:00]未扫描也能显示歌词\n[00:04]第二句本地歌词",
        )
        .expect("write synthetic filename sidecar");
        path.canonicalize().expect("canonical synthetic audio")
    });
    let matched_fixture =
        matched.then(|| matched_local_lyric_fixture::Fixture::new(&root).unwrap());
    let output = root.join(interface_font.unwrap_or(if reopened { "reopen" } else { "first" }));
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
        let font_proof = (fonts || interface_font.is_some()).then(|| Arc::new(FontProof::new(interface_font)));
        if let Some(proof) = &font_proof {
            let proof = proof.clone();
            app.manage(cumulus_desktop::FontCatalogProof::new(move || proof.read()));
        }
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let api=Arc::new(desktop_lyric_fixture::Library::new(credits,matched));
        let proof_store=registry.store();
        let session=Arc::new(AccountSession::new(api.clone(),Some(registry.store())));
        let count=Arc::new(AtomicUsize::new(0));
        let factory_count=count.clone(); let wave=relay_fixture::wave(&root)?;
        let audio_directory=embedded_dir.clone();
        let native_audio=native_path.clone();
        let matched_audio=matched_fixture.clone();
        let client=MusicClient::with_session(session.clone(),registry).with_audio_factory(move || {
            factory_count.fetch_add(1,Ordering::AcqRel);
            if let Some(fixture)=&matched_audio { return fixture.audio(); }
            let audio = relay_fixture::NativeAudio::open(wave.clone())?;
            let audio = match &audio_directory {
                Some(directory) => audio.allow_download_directory(directory.clone()),
                None => audio,
            };
            let audio = match &native_audio {
                Some(path) => audio.allow_native_open(path.clone()),
                None => audio,
            };
            Ok(Box::new(audio))
        });
        app.manage(client);
        app.add_capability(r#"{"identifier":"desktop-lyric-proof","windows":["main","desktop-lyrics-*"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen(if interface_font.is_some() {"ux-proof"} else {"desktop-lyric-proof"}, move |event| {
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
                    if value["key"]=="d" {
                        match shortcut_foreground::grant() {
                            Ok(Some(grant)) => fs::write(output.join(format!("foreground-{id}.json")),serde_json::to_vec_pretty(&grant).unwrap()).unwrap(),
                            Ok(None) => (),
                            Err(error) => { ack(json!({"error":error}));return; }
                        }
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
                "omit_credits" => {
                    assert!(credits);
                    api.omit_credits.store(value["enabled"] == true, Ordering::Release);
                    ack(json!({"ok":true}));
                }
                "credit_calls" => {
                    assert!(credits);
                    ack(json!({"value":&*api.credit_calls.lock().unwrap()}));
                }
                "index_embedded" => {
                    let directory=embedded_dir.as_ref().expect("embedded proof only").clone();
                    let handle=handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let result=handle.state::<MusicClient>().local_command(LocalCommand::Watch {
                            path: directory.to_string_lossy().into_owned(),
                        }).await;
                        let answer=match result {
                            Ok(()) => json!({"ok":true}),
                            Err(_) => json!({"error":"synthetic media could not be indexed"}),
                        };
                        handle.get_webview_window("main").unwrap().eval(format!("window.__lyricAck[{id}]={answer};")).unwrap();
                    });
                }
                "watch_matched" | "link_matched" => {
                    let watch=kind=="watch_matched";
                    let fixture=matched_fixture.as_ref().expect("matched CUE proof only").clone();
                    let handle=handle.clone(); let store=proof_store.clone(); let session=session.clone();
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();
                        let result=if watch {
                            client.local_command(LocalCommand::Watch {
                                path: fixture.directory.to_string_lossy().into_owned(),
                            }).await
                        } else {
                            fixture.link(store,&session)
                        };
                        let answer=match result {
                            Ok(()) => json!({"ok":true}),
                            Err(error) => json!({"error":format!("synthetic CUE index: {error}")}),
                        };
                        handle.get_webview_window("main").unwrap().eval(format!("window.__lyricAck[{id}]={answer};")).unwrap();
                    });
                }
                "open_native_sidecar" => {
                    let path=native_path.as_ref().expect("native sidecar proof only").clone();
                    let handle=handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();
                        let before=client.local_page(LocalQuery::default()).await.unwrap();
                        let epoch=client.account().unwrap().epoch;
                        let result=client.play_native_open_files_warm(&epoch,vec![path],Cancellation::default()).await;
                        let after=client.local_page(LocalQuery::default()).await.unwrap();
                        let answer=match result {
                            Ok(play) => json!({"ok":true,"unindexed":before.tracks.is_empty()&&after.tracks.is_empty(),"playId":play.play_id}),
                            Err(error) => json!({"error":format!("synthetic native open: {error}")}),
                        };
                        handle.get_webview_window("main").unwrap().eval(format!("window.__lyricAck[{id}]={answer};")).unwrap();
                    });
                }
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
                "font_catalog" => {
                    let proof=font_proof.as_ref().expect("font fixture only");
                    if let Some(mode)=value["mode"].as_str() { proof.select(mode); }
                    ack(proof.snapshot());
                }
                "snapshot" | "complete" | "failed" => {
                    let mut value=value.clone(); let output=output.clone(); let handle=handle.clone();
                    value["factoryCount"]=json!(count.load(Ordering::Acquire));
                    value["mainVisible"]=json!(handle.get_webview_window("main").unwrap().is_visible().unwrap());
                    value["lyricWindows"]=json!(handle.webview_windows().keys().filter(|l|l.starts_with("desktop-lyrics-")).collect::<Vec<_>>());
                    value["lyricRequests"]=json!(api.lyrics.load(Ordering::Acquire));
                    value["pendingLyrics"]=json!(api.pending.load(Ordering::Acquire));
                    if let Some(fixture)=&matched_fixture {
                        value["audioReceipt"]=fixture.receipt();
                        value["matchedLyricCalls"]=json!(&*api.matched_calls.lock().unwrap());
                        value["onlineAudioRequests"]=json!(api.matched_audio_requests.load(Ordering::Acquire));
                    }
                    if let Some(proof)=&font_proof {
                        let snapshot=proof.snapshot();
                        value["fontReads"]=snapshot["total"].clone();
                        value["fontSources"]=snapshot["sources"].clone();
                    }
                    #[cfg(target_os="linux")]
                    if let Some(original)=&original_messages {
                        let numeric=locale_name(libc::LC_NUMERIC);
                        let messages=locale_name(libc::LC_MESSAGES);
                        value["locale"]=json!({"numeric":numeric,"messages":messages,"originalMessages":original});
                    }
                    tauri::async_runtime::spawn(async move {
                        let client=handle.state::<MusicClient>();
                        let epoch=client.account().unwrap().epoch;
                        value["nativePlayer"]=serde_json::to_value(client.subscribe_playback(&epoch).await.unwrap().current().unwrap()).unwrap();
                        let stage=value["stage"].as_str().unwrap_or("failed");
                        assert!(stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'));
                        let pending=output.join(format!("{stage}.pending"));
                        fs::write(&pending,serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                        fs::rename(pending,output.join(format!("{stage}.json"))).unwrap();
                        if interface_font.is_some() && !capture_ack::acknowledge(&handle,&output,stage,value["captureHandshake"]==true).await { return; }
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
    }).on_page_load(move |view,payload| {
        if view.label()=="main" && payload.event()==PageLoadEvent::Finished {
            view.eval(format!("window.__lyricProofReopen={};", std::env::var_os("CUMULUS_PROOF_REOPEN").is_some())).unwrap();
            view.eval(format!("window.__nativeSidecarLyricProof={native_sidecar};")).unwrap();
            view.eval(if matched {
                include_str!("matched_local_lyrics_ui_probe.js")
            } else if native_sidecar || std::env::var_os("CUMULUS_EMBEDDED_LYRIC_PROOF").is_some() {
                include_str!("embedded_lyrics_ui_probe.js")
            } else if std::env::var_os("CUMULUS_LYRIC_CREDIT_PROOF").is_some() {
                include_str!("lyric_credit_ui_probe.js")
            } else if let Some(phase) = interface_font {
                view.eval(format!("window.__interfaceFontProofPhase={};", json!(phase))).unwrap();
                let source=include_str!("interface_font_ui_probe.js");
                if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref()==Ok("1") {
                    view.eval(capture_ack::script(source)).unwrap();
                } else {
                    view.eval(source).unwrap();
                }
                return;
            } else if fonts {
                view.eval(format!("window.__fontForceIpcFallback={font_fallback};")).unwrap();
                include_str!("fonts_ui_probe.js")
            } else { include_str!("desktop_lyrics_ui_probe.js") }).unwrap();
        }
    }).run(context).expect("mini WebView proof");
}
