//! Isolated WebView proof for explicitly collecting a mapped local selection.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    library::LocalLibrary,
    playback::engine::{MpvConfig, MpvEngine},
    protocol::{
        api::{Api, ApiFuture, GuardedPlaylistAdd, PlaylistAddPermission},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

#[derive(Default)]
struct Collector {
    calls: Mutex<Vec<Value>>,
    writes: Mutex<Vec<Value>>,
    reveal: AtomicBool,
}

fn profile() -> Value {
    json!({"userId":9,"nickname":"本地收藏合成账号"})
}

fn target(id: u32, owner: u32, special: u32, count: usize) -> Value {
    json!({
        "id":id,"userId":owner,"creator":{"userId":owner},"name":format!("合成歌单 {id}"),
        "specialType":special,"privacy":0,"trackCount":count,"tags":[],
    })
}

impl Api for Collector {
    fn local_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some("sealed-local-batch-favorite".into()))
    }

    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"payload":payload}));
            Ok(match path {
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"account":{"id":9},"profile":profile()})
                }
                "/api/user/playlist" => {
                    json!({"code":200,"more":false,"playlist":[target(81,9,0,0),target(98,9,5,0)]})
                }
                "/api/user/playlist/v2" => {
                    assert_eq!(
                        payload,
                        json!({"uid":"9","offset":0,"limit":10,"sortType":"default"})
                    );
                    json!({"code":200,"more":false,"playlist":[target(81,9,0,0),target(98,9,5,0),target(77,4,0,0)]})
                }
                "/api/v6/playlist/detail" => {
                    assert_eq!(payload["id"], "81");
                    let tracks = if self.reveal.load(Ordering::Acquire) {
                        vec![json!({"id":802}), json!({"id":801})]
                    } else if self.writes.lock().unwrap().is_empty() {
                        vec![]
                    } else {
                        return Err(Error::State("synthetic readback unavailable"));
                    };
                    let mut playlist = target(81, 9, 0, tracks.len());
                    playlist["trackIds"] = json!(tracks);
                    json!({"code":200,"playlist":playlist})
                }
                "/api/v3/song/detail" => {
                    let rows: Value = serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    let id = &rows[0]["id"];
                    assert!(matches!(id.as_str(), Some("801" | "802")));
                    json!({"code":200,"songs":[{"id":id,"name":"合成目录歌曲","privilege":{"subp":-1}}]})
                }
                "/api/song/like/get" => json!({"code":200,"ids":[]}),
                // Optional startup resources stay in the isolated adapter.
                _ => json!({"code":404}),
            })
        })
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            permission()?;
            let (path, payload) = write.into_request()?;
            assert_eq!(path, "/api/v1/playlist/manipulate/tracks");
            assert_eq!(
                payload,
                json!({"pid":"81","trackIds":"[\"802\",\"801\"]","op":"add"})
            );
            dispatched.store(true, Ordering::Release);
            let mut writes = self.writes.lock().unwrap();
            writes.push(payload);
            match writes.len() {
                1 => Err(Error::State("synthetic lost write reply")),
                2 => Ok(json!({"code":502})),
                _ => panic!("a replay escaped the synthetic picker"),
            }
        })
    }
}

fn media(root: &Path) -> std::io::Result<PathBuf> {
    let root = root.join("media");
    fs::create_dir(&root)?;
    let bytes = 8000_u32 * 2 * 30;
    let mut wave = b"RIFF".to_vec();
    wave.extend((bytes + 36).to_le_bytes());
    wave.extend(b"WAVEfmt ");
    wave.extend(16u32.to_le_bytes());
    wave.extend([1, 0, 1, 0]);
    wave.extend(8000u32.to_le_bytes());
    wave.extend(16000u32.to_le_bytes());
    wave.extend([2, 0, 16, 0]);
    wave.extend(b"data");
    wave.extend(bytes.to_le_bytes());
    wave.resize(44 + bytes as usize, 0);
    for name in ["Batch B.wav", "Batch A.wav", "Unmatched.wav"] {
        fs::write(root.join(name), &wave)?;
    }
    Ok(root)
}

#[cfg(target_os = "linux")]
fn locale() -> Value {
    let read = |category| {
        // SAFETY: Query GTK's process locale after startup; copy the pointer immediately.
        unsafe {
            let value = libc::setlocale(category, std::ptr::null());
            assert!(!value.is_null());
            std::ffi::CStr::from_ptr(value)
                .to_string_lossy()
                .into_owned()
        }
    };
    let numeric = read(libc::LC_NUMERIC);
    let messages = read(libc::LC_MESSAGES);
    assert_eq!(numeric, "C");
    assert_eq!(messages, std::env::var("LC_ALL").unwrap());
    assert!(!matches!(messages.as_str(), "C" | "POSIX"));
    json!({"numericLocale":numeric,"messageLocale":messages})
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("proof directory"));
    fs::create_dir(&root).unwrap();
    let media = media(&root).unwrap();
    let out = root.clone();
    let capture = std::env::var_os("CUMULUS_PROOF_CAPTURE_ACK").is_some();
    let script = std::env::var_os("CUMULUS_LOCAL_FAVORITE_SCRIPT")
        .map(fs::read_to_string)
        .transpose()
        .expect("explicit proof script")
        .unwrap_or_else(|| include_str!("local_batch_favorite_ui_probe.js").into());
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            fs::write(out.join("locale.json"), serde_json::to_vec(&locale())?)?;
            let registry = ProfileRegistry::open(&out.join("state"), ApiOrigin::Pc)?;
            let store = registry.store();
            let api = Arc::<Collector>::default();
            let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
            session.adopt(api.clone(), profile(), &session.token()?)?;
            let library = LocalLibrary::new(store)?;
            library.scan(&media)?;
            let client = MusicClient::with_session(session.clone(), registry).with_audio_factory(|| {
                Ok(Box::new(MpvEngine::open(MpvConfig {
                    null_output: true,
                    ..Default::default()
                })?))
            });
            tauri::async_runtime::block_on(async {
                let matching = cumulus_core::library::matching::LocalMatching::new(session, library.clone())?;
                for item in library.catalog(&LocalQuery::default())?.items {
                    let song_id = match item.path.file_name().and_then(|name| name.to_str()) {
                        Some("Batch B.wav") => "802",
                        Some("Batch A.wav") => "801",
                        _ => continue,
                    };
                    matching.bind(&item.id, &item.revision, &json!(song_id)).await?;
                }
                Ok::<(), Error>(())
            })?;
            app.manage(client);
            app.add_capability(r#"{"identifier":"local-favorite-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle = app.handle().clone();
            app.listen("local-favorite-proof", move |event| {
                let mut data: Value = serde_json::from_str(event.payload()).unwrap();
                let stage = data["stage"].as_str().unwrap().to_owned();
                if stage == "reveal_readback" {
                    api.reveal.store(true, Ordering::Release);
                    handle.get_webview_window("main").unwrap().eval("window.__favoriteRevealReady=true").unwrap();
                    return;
                }
                if !matches!(stage.as_str(),
                    "ux_01_local_favorite_picker" | "ux_02_local_favorite_pending" |
                    "ux_03_local_favorite_confirmed" | "ux_04_local_favorite_duplicate" |
                    "complete" | "failed") { return; }
                data["writes"] = json!(*api.writes.lock().unwrap());
                data["collectionReads"] = json!(api.calls.lock().unwrap().iter().filter(|call|
                    matches!(call["path"].as_str(), Some("/api/user/playlist/v2" | "/api/v6/playlist/detail"))
                ).collect::<Vec<_>>());
                fs::write(out.join(format!("{stage}.json")), serde_json::to_vec_pretty(&data).unwrap()).unwrap();
                if stage == "complete" || stage == "failed" {
                    handle.exit(if stage == "complete" { 0 } else { 1 });
                    return;
                }
                if capture {
                    let marker = out.join(format!("{stage}.captured"));
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        if tokio::time::timeout(std::time::Duration::from_secs(10), async {
                            while !marker.is_file() {
                                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                            }
                        }).await.is_ok() {
                            handle.get_webview_window("main").unwrap()
                                .eval(format!("window.__favoriteCaptureAck={};", json!(stage))).unwrap();
                        }
                    });
                }
            });
            Ok(())
        })
        .on_page_load(move |webview, event| {
            if event.event() == PageLoadEvent::Finished {
                webview.eval(format!("window.__favoriteCapture={capture};")).unwrap();
                webview.eval(&script).unwrap();
            }
        })
        .run(tauri::generate_context!())
        .expect("local favorite WebView");
}
