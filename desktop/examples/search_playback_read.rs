//! Fixed search-to-decode proof with a standalone WebView and one private IPC.
#[path = "support/search_download_read.rs"]
mod download_read;
#[path = "support/search_download_transport.rs"]
mod download_transport;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        ClientError, MusicClient,
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
    download: Option<Arc<download_transport::Gate>>,
    live: Option<MusicHttpClient>,
    calls: Mutex<BTreeMap<&'static str, u8>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(gate) = &self.download {
                gate.check_api()?;
                match path {
                    DETAIL => gate.detail(&payload)?,
                    download_transport::PATH => gate.allow_download(&payload)?,
                    _ => (),
                }
            }
            let label = match path {
                ACCOUNT => "account",
                ALL if payload["keyword"] == WORD => "comprehensive",
                SONGS if payload["keyword"] == WORD && payload["offset"] == 0 => "songs",
                DETAIL => "song_detail",
                SOURCE => "audio_source",
                download_transport::PATH if self.download.is_some() => "download_source",
                _ => return Err(Error::InvalidInput("outside fixed proof reads")),
            };
            {
                let mut calls = self.calls.lock().map_err(|_| Error::LockPoisoned)?;
                let count = calls.entry(label).or_default();
                let limit = if path == DETAIL && self.download.is_some() {
                    2
                } else {
                    1
                };
                if *count >= limit {
                    return Err(Error::InvalidInput(
                        "proof does not repeat a service request",
                    ));
                }
                *count += 1;
            }
            let response = if let Some(client) = &self.live {
                client.request(path, payload, cache).await?
            } else {
                let song = || json!({"id":1,"name":"Fixture","dt":1500,"fee":0,"ar":[],"al":{"name":"Fixture"}});
                match path {
                    ACCOUNT => json!({"code":200,"profile":{"userId":2,"nickname":"Fixture"}}),
                    ALL => {
                        json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":song()}]}]}})
                    }
                    SONGS => {
                        json!({"code":200,"data":{"resources":[{"baseInfo":{"simpleSongData":song()}}],"totalCount":1}})
                    }
                    DETAIL => {
                        json!({"code":200,"songs":[song()],"privileges":[{"id":1,"fee":0,"st":0,"dl":128000}]})
                    }
                    SOURCE => {
                        json!({"code":200,"data":[{"id":1,"url":FIXTURE_URL,"code":200,"md5":"synthetic","level":"standard","expi":120}]})
                    }
                    download_transport::PATH => {
                        json!({"code":200,"data":{"id":1,"url":download_transport::URL,"code":200,"type":"flac","size":SEED.len(),"md5":download_transport::MD5,"level":"standard"}})
                    }
                    _ => return Err(Error::InvalidInput("outside fixed proof reads")),
                }
            };
            if let Some(gate) = &self.download {
                match path {
                    ACCOUNT => gate.identity(&response)?,
                    DETAIL => gate.record_detail(&response)?,
                    download_transport::PATH => gate.receipt(&response)?,
                    _ => (),
                }
            }
            Ok(response)
        })
    }
}

struct Audio {
    engine: MpvEngine,
    fixture: Option<PathBuf>,
    offline_files: Option<(PathBuf, Arc<download_transport::Gate>)>,
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
        if let Some((root, gate)) = &self.offline_files
            && gate.facts()?.1
        {
            let MediaInput::File(file) = &request.source else {
                return Err(Error::InvalidInput("offline proof requires a file"));
            };
            let canonical = file.canonicalize()?;
            if canonical.parent() != Some(root.as_path()) || !canonical.is_file() {
                return Err(Error::InvalidInput(
                    "offline proof source escaped download directory",
                ));
            }
            request.source = MediaInput::File(canonical);
            return self.engine.load(request, cancel);
        }
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

struct DownloadProof {
    gate: Arc<download_transport::Gate>,
    directory: PathBuf,
    store: Arc<StateStore>,
}

struct Proof {
    download: Option<DownloadProof>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    download: Option<download_read::DownloadFacts>,
}

async fn verify(proof: &Proof) -> std::result::Result<Facts, &'static str> {
    let client = &proof.client;
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
        channel: None,
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
    if let Some(download) = &proof.download {
        download
            .gate
            .choose(&song.id)
            .map_err(|_| "download_selection")?;
    }
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
        attempted.map_err(|error| match ClientError::from(error).code {
            "playback_source_unavailable" => "no_authorized_source",
            "audio_open_failed" => "authorized_media_open",
            "audio_output_failed" => "native_audio_output",
            "audio_unavailable" => "native_audio_runtime",
            "network" => "source_request_network",
            "session_expired" => "source_session_expired",
            _ => "native_selection",
        })?;
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
    let download = if let Some(download) = &proof.download {
        let owner = download.gate.owner().map_err(|_| "download_owner")?;
        let facts = download_read::verify_download(
            client,
            &epoch,
            &song.id,
            &download.directory,
            &download.store,
            &owner,
            || async { download.gate.cutoff() },
        )
        .await?;
        if download.gate.facts().map_err(|_| "download_gate")? != (1, true, 0) {
            return Err("offline_proof_attempted_network");
        }
        Some(facts)
    } else {
        None
    };
    Ok(Facts {
        comprehensive_sections: all.sections.len(),
        songs: songs.len(),
        clock_advanced: true,
        stopped: true,
        download,
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
    let outcome = tokio::time::timeout(Duration::from_secs(100), verify(&proof))
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
    let mode = args.next().expect("proof mode");
    let (source, with_download) = match mode.to_str() {
        Some("--synthetic") => (None, false),
        Some("--synthetic-download") => (None, true),
        Some("--live" | "--live-download") => (
            Some(PathBuf::from(
                args.next().expect("closed authorized PC profile copy"),
            )),
            mode == "--live-download",
        ),
        _ => panic!("expected a fixed synthetic or live proof mode"),
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
            let gate = with_download
                .then(|| download_transport::Gate::new(!synthetic).map(Arc::new))
                .transpose()?;
            let reads = Arc::new(Reads {
                live,
                download: gate.clone(),
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
            let download = if let Some(gate) = gate {
                let directory = root.join("downloaded");
                private_directory_builder().create(&directory)?;
                Some(DownloadProof {
                    gate,
                    directory,
                    store: registry.store(),
                })
            } else {
                None
            };
            let offline_files = download
                .as_ref()
                .map(|proof| (proof.directory.clone(), proof.gate.clone()));
            let client =
                MusicClient::with_session(session, registry).with_audio_factory(move || {
                    Ok(Box::new(Audio {
                        engine: MpvEngine::open(MpvConfig {
                            null_output: true,
                            ..Default::default()
                        })?,
                        fixture: fixture.clone(),
                        offline_files: offline_files.clone(),
                    }))
                });
            let client = if let Some(proof) = &download {
                client.with_download_backend(proof.gate.clone())
            } else {
                client
            };
            let webview_path = root.join("webview");
            app.manage(Proof {
                client,
                download,
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
