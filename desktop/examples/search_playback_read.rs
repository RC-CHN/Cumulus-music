//! Fixed search-to-decode proof with a standalone WebView and one private IPC.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction, SearchCategory, SearchItem, SearchResultQuery},
    },
    membership::Quality,
    playback::{controller::PlaybackStatus, engine::*},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::{StateStore, private_directory_builder},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    Manager,
    utils::{
        assets::{AssetKey, AssetsIter, CspHash},
        config::Csp,
    },
};

const LABEL: &str = "search-playback-read";
const WORD: &str = "夜空中最亮的星 逃跑计划";
const FIXTURE_URL: &str = "https://m1.music.126.net/cumulus-proof-only.flac";
const ACCOUNT: &str = "/api/w/nuser/account/get";
const ALL: &str = "/api/search/pc/complex/page";
const SONGS: &str = "/api/search/song/list/page";
const DETAIL: &str = "/api/v3/song/detail";
const SOURCE: &str = "/api/song/enhance/player/url/v1";
const SEED: &[u8] = include_bytes!("../../core/tests/fixtures/tag_audio/seed.flac");

struct Assets;
impl tauri::Assets<tauri::Wry> for Assets {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        let asset = ASSETS.iter().find(|(name, _)| *name == key.as_ref())?;
        Some(Cow::Borrowed(asset.1))
    }
    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(
            ASSETS
                .iter()
                .map(|(name, bytes)| (Cow::Borrowed(*name), Cow::Borrowed(*bytes))),
        )
    }
    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}
const ASSETS: &[(&str, &[u8])] = &[
    ("/index.html", br#"<!doctype html><html lang="en"><meta charset="utf-8"><title>Search playback read proof</title><main><h1>Search playback read proof</h1><p id="status" role="status">Checking fixed search and playback...</p></main><script defer src="/probe.js"></script></html>"#),
    ("/probe.js", br#"'use strict'; const status = document.getElementById('status'); window.__TAURI_INTERNALS__.invoke('run_search_playback_read').then(ok => { status.textContent = ok ? 'Verified' : 'Failed'; }).catch(() => { status.textContent = 'Unavailable'; });"#),
];

struct Reads {
    live: Option<MusicHttpClient>,
    calls: Mutex<BTreeMap<&'static str, u8>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let label = match path {
                ACCOUNT => "account",
                ALL if payload["keyword"] == WORD => "comprehensive",
                SONGS if payload["keyword"] == WORD && payload["offset"] == 0 => "songs",
                DETAIL => "song_detail",
                SOURCE => "audio_source",
                _ => return Err(Error::InvalidInput("outside fixed proof reads")),
            };
            {
                let mut calls = self.calls.lock().map_err(|_| Error::LockPoisoned)?;
                let count = calls.entry(label).or_default();
                if *count != 0 {
                    return Err(Error::InvalidInput(
                        "proof does not repeat a service request",
                    ));
                }
                *count += 1;
            }
            if let Some(client) = &self.live {
                return client.request(path, payload, cache).await;
            }
            let song =
                || json!({"id":1,"name":"Fixture","dt":1500,"ar":[],"al":{"name":"Fixture"}});
            Ok(match path {
                ACCOUNT => json!({"code":200,"profile":{"userId":2,"nickname":"Fixture"}}),
                ALL => {
                    json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":song()}]}]}})
                }
                SONGS => {
                    json!({"code":200,"data":{"resources":[{"baseInfo":{"simpleSongData":song()}}],"totalCount":1}})
                }
                DETAIL => json!({"code":200,"songs":[song()]}),
                SOURCE => {
                    json!({"code":200,"data":[{"id":1,"url":FIXTURE_URL,"code":200,"md5":"synthetic","level":"standard","expi":120}]})
                }
                _ => return Err(Error::InvalidInput("outside fixed proof reads")),
            })
        })
    }
}

struct Audio {
    engine: MpvEngine,
    fixture: Option<PathBuf>,
}
fn allowed_audio(input: &str) -> bool {
    tauri::Url::parse(input).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.host_str().is_some_and(|host| {
                host.ends_with(".music.126.net") || host.ends_with(".music.163.com")
            })
    })
}
impl AudioEngine for Audio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        match (&self.fixture, &request.source) {
            (Some(file), MediaInput::Url(url)) if url.as_str() == FIXTURE_URL => {
                request.source = MediaInput::File(file.clone());
            }
            (None, MediaInput::Url(url)) if allowed_audio(url.as_str()) => {}
            _ => {
                return Err(Error::InvalidInput(
                    "proof audio is outside its allowed initial origin",
                ));
            }
        }
        self.engine.load(request, cancel)
    }
    fn pause(&mut self, value: bool) -> Result<()> {
        self.engine.pause(value)
    }
    fn seek(&mut self, value: f64) -> Result<()> {
        self.engine.seek(value)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.engine.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.engine.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.engine.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.engine.poll()
    }
}

struct Proof {
    client: MusicClient,
    reads: Arc<Reads>,
    started: AtomicBool,
    root: PathBuf,
    synthetic: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    comprehensive_sections: usize,
    songs: usize,
    clock_advanced: bool,
    stopped: bool,
}

async fn verify(client: &MusicClient) -> std::result::Result<Facts, &'static str> {
    let account = client.restore_account().await.map_err(|_| "account_read")?;
    if account.status != cumulus_core::account::SessionStatus::Authenticated {
        return Err("session_not_authenticated");
    }
    let epoch = account.epoch;
    let query = |category| SearchResultQuery {
        keyword: WORD.into(),
        category,
        offset: 0,
        limit: 6,
        correct: true,
    };
    let all = client
        .search_results(&epoch, query(SearchCategory::All))
        .await
        .map_err(|_| "comprehensive_read")?
        .value;
    if all.sections.is_empty() {
        return Err("comprehensive_empty");
    }
    let page = client
        .search_results(&epoch, query(SearchCategory::Songs))
        .await
        .map_err(|_| "songs_read")?
        .value;
    let songs: Vec<_> = page
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .filter_map(|item| match item {
            SearchItem::Song { track, .. } if !track.missing && track.duration_ms >= 1_000 => {
                Some(track)
            }
            _ => None,
        })
        .collect();
    let song = songs.first().ok_or("no_concrete_song")?;
    let attempted = client
        .play_selection(
            &epoch,
            PlaySelection::Songs {
                ids: vec![song.id.clone()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await;
    let observation = async {
        attempted.map_err(|_| "native_selection")?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut first = None;
        loop {
            let current = client
                .player_snapshot(&epoch)
                .await
                .map_err(|_| "native_snapshot")?;
            if current.error.is_some() {
                return Err("native_playback");
            }
            if current.state == PlaybackStatus::Playing
                && current.queue_len == 1
                && current
                    .track
                    .as_ref()
                    .is_some_and(|track| track.id == song.id)
                && let Some(clock) = current.position
                && clock - *first.get_or_insert(clock) > 0.2
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("playback_clock_did_not_advance");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    .await;
    let stop = client.player_action(&epoch, PlayerAction::Stop).await;
    observation?;
    if !stop.is_ok_and(|view| view.state == PlaybackStatus::Idle) {
        return Err("native_stop");
    }
    Ok(Facts {
        comprehensive_sections: all.sections.len(),
        songs: songs.len(),
        clock_advanced: true,
        stopped: true,
    })
}

#[tauri::command]
async fn run_search_playback_read(
    window: tauri::WebviewWindow,
    proof: tauri::State<'_, Proof>,
) -> std::result::Result<bool, &'static str> {
    if window.label() != LABEL || proof.started.swap(true, Ordering::AcqRel) {
        return Err("fixed proof already invoked or unavailable");
    }
    let outcome = tokio::time::timeout(Duration::from_secs(100), verify(&proof.client))
        .await
        .unwrap_or(Err("deadline"));
    let calls = proof
        .reads
        .calls
        .lock()
        .map_err(|_| "proof accounting")?
        .clone();
    let ok = outcome.is_ok();
    let report = json!({"ok":ok, "stage":outcome.as_ref().err(), "facts":outcome.ok(), "reads":calls,
        "synthetic":proof.synthetic, "realAccountWrites":0, "actualWebView":true, "nullOutput":true});
    fs::write(
        proof.root.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|_| "proof encoding")?,
    )
    .map_err(|_| "proof output")?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        app.exit(if ok { 0 } else { 1 });
    });
    Ok(ok)
}

fn copy_profile(source: &Path, destination: &Path) -> std::io::Result<()> {
    let invalid = || std::io::Error::other("expected a closed standalone PC profile copy");
    if !fs::symlink_metadata(source)?.is_dir() {
        return Err(invalid());
    }
    let db = source.join("core.sqlite3");
    if !fs::symlink_metadata(&db)?.is_file() {
        return Err(invalid());
    }
    for suffix in ["-journal", "-wal", "-shm"] {
        match fs::symlink_metadata(source.join(format!("core.sqlite3{suffix}"))) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(invalid()),
        }
    }
    private_directory_builder().create(destination)?;
    fs::copy(db, destination.join("core.sqlite3"))?;
    Ok(())
}

fn main() {
    // SAFETY: first action, before GTK and native workers.
    unsafe { cumulus_desktop::prepare_graphics() };
    let mut args = std::env::args_os().skip(1);
    let source = match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--synthetic") => None,
        Some("--live") => Some(PathBuf::from(
            args.next().expect("closed authorized PC profile copy"),
        )),
        _ => panic!("expected --synthetic NEW_ROOT or --live CLOSED_PC_PROFILE_COPY NEW_ROOT"),
    };
    assert!(
        source.is_none() || std::env::var_os("CI").is_none(),
        "live account proof is disabled in CI"
    );
    let root = PathBuf::from(args.next().expect("new private proof directory"));
    assert!(args.next().is_none(), "unexpected arguments");
    private_directory_builder()
        .create(&root)
        .expect("new private proof directory");
    let root = root
        .canonicalize()
        .expect("resolve private proof directory");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().app.security.csp = Some(Csp::Policy(
        "default-src 'none'; script-src 'self'; connect-src ipc: http://ipc.localhost; frame-ancestors 'none'".into(),
    ));
    context.set_assets(Box::new(Assets));
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_search_playback_read])
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            {
                // SAFETY: Tauri setup is after GTK initialization, before player
                // workers, with a static category value and string.
                if unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) }.is_null() {
                    return Err(std::io::Error::other("numeric locale setup failed").into());
                }
            }
            let profile = root.join("credential-copy");
            let live = if let Some(source) = source {
                copy_profile(&source, &profile)?;
                let credentials = Arc::new(Credentials::open(
                    Arc::new(StateStore::open(&profile)?),
                    ApiOrigin::Pc,
                )?);
                if credentials.login_credentials()?.token.is_none() {
                    return Err(
                        Error::InvalidInput("existing authorized PC credentials required").into(),
                    );
                }
                Some(MusicHttpClient::new(credentials, ClientProfile::default())?)
            } else {
                None
            };
            let synthetic = live.is_none();
            let reads = Arc::new(Reads {
                live,
                calls: Mutex::default(),
            });
            let registry = ProfileRegistry::open(&root.join("local-state"), ApiOrigin::Pc)?;
            let session = Arc::new(AccountSession::new(reads.clone(), Some(registry.store())));
            let fixture = if synthetic {
                let file = root.join("synthetic.flac");
                fs::write(&file, SEED)?;
                Some(file)
            } else {
                None
            };
            let client =
                MusicClient::with_session(session, registry).with_audio_factory(move || {
                    Ok(Box::new(Audio {
                        engine: MpvEngine::open(MpvConfig {
                            null_output: true,
                            ..Default::default()
                        })?,
                        fixture: fixture.clone(),
                    }))
                });
            let webview_path = root.join("webview");
            app.manage(Proof {
                client,
                reads,
                started: AtomicBool::new(false),
                root,
                synthetic,
            });
            tauri::WebviewWindowBuilder::new(
                app,
                LABEL,
                tauri::WebviewUrl::App("index.html".into()),
            )
            .data_directory(webview_path)
            .title("Fixed search playback read proof")
            .inner_size(640.0, 320.0)
            .build()?;
            Ok(())
        })
        .run(context)
        .expect("fixed search/playback proof");
}
