//! Isolated native download records for UI inspection. No remote media transfer.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    client::{MusicClient, dto::DownloadCommand},
    download::{
        service::{
            DownloadDestination, DownloadOptions, DownloadOrigin, DownloadPhase,
            DownloadRunOptions, DownloadService, DownloadStatus,
        },
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    membership::Quality,
    protocol::api::ApiFuture,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_DOWNLOAD_PAGE_UI_PROOF").is_some()
}
pub fn quality_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_DOWNLOAD_QUALITY_UI_PROOF").is_some()
}
pub fn menu_comments_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_DOWNLOAD_MENU_COMMENT_PROOF").is_some()
}
static HOLD_MENU_COMMENT: AtomicBool = AtomicBool::new(false);
static MENU_COMMENT_HELD: AtomicBool = AtomicBool::new(false);
static MENU_COMMENT_CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static RETIRED_MENU_TASK: Mutex<Option<(String, Value)>> = Mutex::new(None);
static CLOUD_MENU_TASKS: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
static CLOUD_MENU_ACTIVE: AtomicBool = AtomicBool::new(false);
pub fn menu_comment_held() -> bool {
    MENU_COMMENT_HELD.load(Ordering::SeqCst)
}
pub fn menu_comment_calls() -> Vec<Value> {
    MENU_COMMENT_CALLS.lock().unwrap().clone()
}
pub async fn menu_comment_response(
    path: &str,
    args: &Value,
    cached: bool,
) -> Option<Result<Value>> {
    if !menu_comments_enabled() || path != "/api/resource/commentInfo/list" {
        return None;
    }
    let ids = args["resourceIds"].as_str()?;
    let (song, thread, count) = match ids {
        "[\"1\"]" => (1, "R_SO_4_1", 321),
        "[\"2\"]" => (2, "R_SO_4_2", 0),
        "[\"3\"]" => (3, "R_SO_4_4", 987),
        "[\"9010\"]" if CLOUD_MENU_ACTIVE.load(Ordering::SeqCst) => (9010, "R_SO_4_9010", 654),
        "[\"5\"]" | "[\"6\"]" | "[\"7\"]" if CLOUD_MENU_ACTIVE.load(Ordering::SeqCst) => {
            MENU_COMMENT_CALLS.lock().unwrap().push(args.clone());
            return Some(Err(Error::InvalidInput(
                "cloud file ID reached the comment API",
            )));
        }
        _ => return None,
    };
    assert_eq!(*args, json!({"resourceType":4,"resourceIds":ids}));
    assert!(!cached);
    MENU_COMMENT_CALLS.lock().unwrap().push(args.clone());
    if song == 1 && HOLD_MENU_COMMENT.load(Ordering::SeqCst) {
        struct Held;
        impl Drop for Held {
            fn drop(&mut self) {
                MENU_COMMENT_HELD.store(false, Ordering::SeqCst);
            }
        }
        let _held = Held;
        MENU_COMMENT_HELD.store(true, Ordering::SeqCst);
        while HOLD_MENU_COMMENT.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Some(Ok(
        json!({"code":200,"data":[{"threadId":thread,"commentCount":count}]}),
    ))
}
pub fn voice_podcast_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_DOWNLOAD_VOICE_PODCAST_PROOF").is_some()
}
static VOICE_PODCAST_READS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const VOICE_RADIO_ID: &str = "9007199254740993";
fn voice_radio() -> Value {
    json!({"id":VOICE_RADIO_ID,"name":"合成下载节目所属播客","programCount":1})
}
fn voice_episode(episode_id: u64, media_id: u64) -> Value {
    json!({"id":episode_id,"name":format!("合成声音 {media_id:03}"),
        "mainSong":{"id":media_id},"duration":30000,"radio":voice_radio(),
        "dj":{"userId":88,"nickname":"合成主播（不是播客名）"}})
}
pub fn voice_podcast_report() -> Value {
    json!(*VOICE_PODCAST_READS.lock().unwrap())
}
static QUALITY_MEMBERSHIP_READS: AtomicUsize = AtomicUsize::new(0);
static QUALITY_STARTUP_READ: tokio::sync::Notify = tokio::sync::Notify::const_new();
static QUALITY_DOWNLOAD_READS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static DATA: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
static SEED_LYRIC_FAILURE: AtomicBool = AtomicBool::new(false);
static CHUNK: AtomicUsize = AtomicUsize::new(512);
static MV_DETAIL: AtomicUsize = AtomicUsize::new(0);
static MV_STREAM: AtomicUsize = AtomicUsize::new(0);
const MEDIA: &str = "https://m1.music.126.net/cumulus-download-fixture.wav";
pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if voice_podcast_enabled() {
        let response = match path {
            "/api/djradio/v3/get" => {
                assert_eq!(*args, json!({"id":VOICE_RADIO_ID}));
                Some(json!({"code":200,"data":voice_radio()}))
            }
            "/api/v6/dj/program/byradio" => {
                assert_eq!(
                    *args,
                    json!({"radioId":VOICE_RADIO_ID,"limit":200,"offset":0,"asc":true})
                );
                Some(
                    json!({"code":200,"data":{"programs":[voice_episode(701,121)],"count":1,
                    "more":false,"asc":args["asc"]}}),
                )
            }
            "/api/content/my/liked/voice" => {
                assert_eq!(*args, json!({"limit":100,"likeTime":0}));
                Some(json!({"code":200,"data":{"data":[],"hasMore":false}}))
            }
            "/api/dj/user/setting/resumeplay/get" => {
                assert_eq!(*args, json!({"radioId":VOICE_RADIO_ID}));
                Some(json!({"code":200,"data":{"resumePlayValue":0}}))
            }
            _ => None,
        };
        if let Some(response) = response {
            VOICE_PODCAST_READS
                .lock()
                .unwrap()
                .push(json!({"path":path,"args":args}));
            return Some(Ok(response));
        }
    }
    if quality_enabled() && path == "/api/music-vip-membership/client/vip/info" {
        assert_eq!(*args, json!({"userId":"2"}));
        let attempt = QUALITY_MEMBERSHIP_READS.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            QUALITY_STARTUP_READ.notify_one();
        }
        return Some(match attempt {
            0 | 1 => Err(Error::Transport("synthetic membership read failed".into())),
            2 => Ok(
                json!({"code":200,"data":{"userId":2,"now":1_900_000_000_000u64,
                "associator":{"expireTime":1_900_086_400_000u64,"vipLevel":7}}}),
            ),
            _ => panic!("unrequested synthetic membership refresh"),
        });
    }
    if enabled() && path == "/api/mlog/detail/v1" {
        MV_DETAIL.fetch_add(1, Ordering::SeqCst);
        return Some(Err(Error::Transport(
            "synthetic offline MV metadata".into(),
        )));
    }
    if enabled() && path == "/api/song/enhance/play/mv/url" {
        MV_STREAM.fetch_add(1, Ordering::SeqCst);
        return Some(Err(Error::State(
            "offline MV must not request a streaming URL",
        )));
    }
    if enabled()
        && path == "/api/song/lyric/v1"
        && (args["id"] == 1 || args["id"] == "1")
        && SEED_LYRIC_FAILURE.load(Ordering::Relaxed)
    {
        return Some(Err(Error::Transport("synthetic lyric read failed".into())));
    }
    if !enabled() || path != "/api/song/enhance/download/url/v1" {
        return None;
    }
    if quality_enabled() {
        assert_eq!(
            *args,
            json!({"id":"1","level":"exhigh","immerseType":"c51"})
        );
        QUALITY_DOWNLOAD_READS.lock().unwrap().push(args.clone());
    }
    let data = DATA.get().expect("seed first");
    Some(Ok(
        json!({"code":200,"data":{"id":args["id"],"code":200,"url":MEDIA,"type":"wav","size":data.len(),"level":"standard"}}),
    ))
}
pub fn quality_report(store: &StateStore) -> Result<Value> {
    let stored = store.get("2", "download_preferences_v1", "options")?;
    let tasks = store.values("2", "download_tasks_v1")?;
    let tasks: Vec<_> = tasks
        .values()
        .map(|task| {
            let published = task["status"] == "complete"
                && task["target"].as_str().is_some_and(|target| {
                    std::fs::read(target).is_ok_and(|bytes| {
                        DATA.get()
                            .is_some_and(|seed| bytes.as_slice() == seed.as_slice())
                    })
                });
            json!({"songId":task["song_id"],"status":task["status"],
                "requested":task["options"]["quality"],"actual":task["resource"]["level"],
                "authorizations":task["authorization_count"],"published":published})
        })
        .collect();
    Ok(
        json!({"membershipReads":QUALITY_MEMBERSHIP_READS.load(Ordering::SeqCst),
        "downloadReads":*QUALITY_DOWNLOAD_READS.lock().unwrap(),
        "savedQuality":stored.map(|row| row["quality"].clone()),"tasks":tasks}),
    )
}
pub struct NoNetwork;
impl MediaBackend for NoNetwork {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str() != MEDIA {
                return Err(Error::State("download proof forbids remote media"));
            }
            let data = DATA.get().expect("seed first").clone();
            let offset = usize::try_from(request.offset).unwrap();
            assert!(offset < data.len());
            let mut headers = std::collections::BTreeMap::from([
                ("content-length".into(), (data.len() - offset).to_string()),
                ("etag".into(), "\"synthetic-wave\"".into()),
            ]);
            if offset > 0 {
                headers.insert(
                    "content-range".into(),
                    format!("bytes {offset}-{}/{}", data.len() - 1, data.len()),
                );
            }
            Ok(MediaReply {
                status: if offset == 0 { 200 } else { 206 },
                headers,
                body: Box::new(Body { data, offset }),
            })
        })
    }
}
struct Body {
    data: Arc<Vec<u8>>,
    offset: usize,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let size = buffer
                .len()
                .min(CHUNK.load(Ordering::Relaxed))
                .min(self.data.len() - self.offset);
            buffer[..size].copy_from_slice(&self.data[self.offset..self.offset + size]);
            self.offset += size;
            Ok(size)
        })
    }
}
pub struct Fixture {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    root: PathBuf,
    records: Mutex<Vec<(String, Value)>>,
}
impl Fixture {
    pub fn new(session: Arc<AccountSession>, store: Arc<StateStore>, root: PathBuf) -> Self {
        Self {
            session,
            store,
            root,
            records: Mutex::new(vec![]),
        }
    }
    pub async fn control(&self, client: &MusicClient, value: &Value) -> Result<()> {
        assert!(enabled() && self.store.directory().starts_with(&self.root));
        let started = std::time::Instant::now();
        let mode = value["mode"]
            .as_str()
            .ok_or(Error::InvalidInput("missing synthetic mode"))?;
        eprintln!("download UI fixture {mode}: started");
        let epoch = client.account()?.epoch;
        match value["mode"].as_str().unwrap() {
            "menu_comment_hold" if menu_comments_enabled() => {
                HOLD_MENU_COMMENT.store(true, Ordering::SeqCst);
            }
            "menu_comment_release" if menu_comments_enabled() => {
                HOLD_MENU_COMMENT.store(false, Ordering::SeqCst);
            }
            "menu_comment_observe" if menu_comments_enabled() => {}
            "menu_comment_retire" if menu_comments_enabled() => {
                assert!(menu_comment_held());
                let mut retired = RETIRED_MENU_TASK.lock().unwrap();
                assert!(retired.is_none());
                let task = self.records.lock().unwrap()[0].0.clone();
                let record = self
                    .store
                    .get("2", "download_tasks_v1", &task)?
                    .ok_or(Error::State("missing synthetic menu task"))?;
                self.store.delete("2", "download_tasks_v1", &task)?;
                *retired = Some((task, record));
            }
            "menu_comment_restore" if menu_comments_enabled() => {
                let (task, record) = RETIRED_MENU_TASK
                    .lock()
                    .unwrap()
                    .take()
                    .ok_or(Error::State("no retired synthetic menu task"))?;
                self.store.put("2", "download_tasks_v1", &task, &record)?;
            }
            "cloud_comment_targets" if menu_comments_enabled() => {
                let mut saved = CLOUD_MENU_TASKS.lock().unwrap();
                assert!(saved.is_empty());
                let tasks = self.records.lock().unwrap();
                for (index, song, public) in
                    [(4, "5", Some("9010")), (5, "6", None), (6, "7", None)]
                {
                    let id = tasks[index].0.clone();
                    let mut task = self
                        .store
                        .get("2", "download_tasks_v1", &id)?
                        .ok_or(Error::State("missing synthetic cloud comment task"))?;
                    assert_eq!(task["song_id"], song);
                    assert_eq!(task["status"], "complete");
                    saved.push((id.clone(), task.clone()));
                    task["origin"] = json!({"kind":"cloud","file_name":format!("private-{song}.wav"),
                        "file_size":task["bytes"]});
                    if let Some(public) = public {
                        task["song"]["_cloud_catalog_id"] = json!(public);
                        task["song"]["_cloud_public_id"] = json!(public);
                    } else if song == "6" {
                        task["song"]["_cloud_catalog_id"] = json!("9011");
                        task["song"]["_cloud_public_id"] = Value::Null;
                    }
                    self.store.put("2", "download_tasks_v1", &id, &task)?;
                }
                CLOUD_MENU_ACTIVE.store(true, Ordering::SeqCst);
            }
            "cloud_comment_restore" if menu_comments_enabled() => {
                let mut saved = CLOUD_MENU_TASKS.lock().unwrap();
                assert_eq!(saved.len(), 3);
                for (task, record) in saved.drain(..) {
                    self.store.put("2", "download_tasks_v1", &task, &record)?;
                }
                CLOUD_MENU_ACTIVE.store(false, Ordering::SeqCst);
            }
            "seed_queue_collection"
                if std::env::var_os("CUMULUS_QUEUE_ROW_COLLECTION_PROOF").is_some() =>
            {
                assert_eq!(self.session.token()?.uid.as_deref(), Some("2"));
                assert!(self.store.values("2", "download_tasks_v1")?.is_empty());
                let media = self.root.join("media");
                client
                    .download_command(
                        &epoch,
                        DownloadCommand::Configure {
                            root: media.to_string_lossy().into_owned(),
                            path_type: 0,
                            file_type: 0,
                        },
                    )
                    .await?;
                let wave = self.root.join("relay-synthetic.wav");
                DATA.set(Arc::new(std::fs::read(&wave)?)).unwrap();
                CHUNK.store(usize::MAX, Ordering::Relaxed);
                let service = DownloadService::with_backend(
                    self.session.clone(),
                    Arc::new(NoNetwork),
                    64 * 1024 * 1024,
                )?;
                let task = service.create(
                    &json!("7"),
                    DownloadDestination::Path {
                        path: media.join("published-queue-song.wav"),
                    },
                    DownloadOptions {
                        tags: false,
                        cover: false,
                        ..Default::default()
                    },
                )?;
                let published = service.run(&task.id, DownloadRunOptions::default()).await?;
                assert_eq!(published.status, DownloadStatus::Complete);
                assert_eq!(published.origin, DownloadOrigin::Song);

                let store = self.store.clone();
                let session = self.session.clone();
                tokio::task::spawn_blocking(move || {
                    let local = media.join("collection-library");
                    std::fs::create_dir(&local)?;
                    let mapped = local.join("linked-queue-song.wav");
                    std::fs::copy(&wave, &mapped)?;
                    std::fs::copy(wave, local.join("unlinked-queue-song.wav"))?;
                    let library = cumulus_core::library::LocalLibrary::new(store)?;
                    library.scan(&local)?;
                    let item = library
                        .list(Default::default())?
                        .into_iter()
                        .find(|item| item.path.file_name() == mapped.file_name())
                        .ok_or(Error::State("synthetic indexed track missing"))?;
                    library.map_online(&session, &item.id, &json!("8"))
                })
                .await
                .map_err(|_| Error::State("synthetic local library worker stopped"))??;
            }
            "seed_quality" if quality_enabled() => {
                assert_eq!(self.session.token()?.uid.as_deref(), Some("2"));
                assert!(self.store.values("2", "download_tasks_v1")?.is_empty());
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    QUALITY_STARTUP_READ.notified(),
                )
                .await
                .map_err(|_| Error::State("synthetic membership startup read did not dispatch"))?;
                assert_eq!(QUALITY_MEMBERSHIP_READS.load(Ordering::SeqCst), 1);
                client
                    .download_command(
                        &epoch,
                        DownloadCommand::Configure {
                            root: self.root.join("media").to_string_lossy().into_owned(),
                            path_type: 0,
                            file_type: 0,
                        },
                    )
                    .await?;
                let wave = self.root.join("relay-synthetic.wav");
                let wave = if wave.is_file() {
                    wave
                } else {
                    super::relay_fixture::wave_seconds(&self.root, 1)?
                };
                DATA.set(Arc::new(std::fs::read(wave)?)).unwrap();
                CHUNK.store(usize::MAX, Ordering::Relaxed);
                self.store.put(
                    "2",
                    "download_preferences_v1",
                    "options",
                    &serde_json::to_value(DownloadOptions {
                        quality: Quality::Vivid,
                        tags: false,
                        cover: false,
                        ..Default::default()
                    })?,
                )?;
            }
            "seed" => {
                assert!(self.records.lock().unwrap().is_empty());
                let delay = std::env::var("CUMULUS_DOWNLOAD_SEED_DELAY_MS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|ms| *ms <= 30_000)
                    .unwrap_or(0);
                if delay > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
                let media = self.root.join("media");
                client
                    .download_command(
                        &epoch,
                        DownloadCommand::Configure {
                            root: media.to_string_lossy().into_owned(),
                            path_type: 0,
                            file_type: 0,
                        },
                    )
                    .await?;
                let service = DownloadService::with_backend(
                    self.session.clone(),
                    Arc::new(NoNetwork),
                    64 * 1024 * 1024,
                )?;
                let wave = super::relay_fixture::wave(&self.root)?;
                let integrity = cumulus_core::media::integrity::inspect_file(&wave)?;
                let offline =
                    cumulus_core::download::offline::OfflineLibrary::new(self.session.clone())?;
                let token = self.session.token()?;
                DATA.set(Arc::new(std::fs::read(&wave)?)).unwrap();
                let mut saved = vec![];
                let lyric_repair =
                    std::env::var_os("CUMULUS_DOWNLOAD_LYRIC_REPAIR_PROOF").is_some();
                for i in 1..=146_u64 {
                    let path = media.join(format!(
                        "synthetic-{i}.{}",
                        if i == 133 { "mp4" } else { "wav" }
                    ));
                    let mut task = service.create(
                        &json!(i),
                        DownloadDestination::Path { path: path.clone() },
                        DownloadOptions {
                            tags: false,
                            cover: false,
                            lyrics: lyric_repair && i == 1,
                            ..Default::default()
                        },
                    )?;
                    // Service creation canonicalizes the destination, including
                    // Windows' verbatim prefix. Completed records must keep it.
                    let path = match &task.destination {
                        DownloadDestination::Path { path } => path.clone(),
                        _ => unreachable!("synthetic path destination"),
                    };
                    let (kind, origin) = if i <= 120 {
                        (
                            "单曲",
                            if i == 120 {
                                DownloadOrigin::Cloud {
                                    file_name: "original.wav".into(),
                                    file_size: 2800000,
                                }
                            } else {
                                DownloadOrigin::Song
                            },
                        )
                    } else if i <= 132 {
                        (
                            "声音",
                            DownloadOrigin::Voice {
                                voice_id: i.to_string(),
                            },
                        )
                    } else if i <= 141 {
                        ("MV", DownloadOrigin::Mv { resolution: 1080 })
                    } else {
                        ("任务", DownloadOrigin::Song)
                    };
                    task.origin = origin;
                    task.song = Some(
                        json!({"id":i,"name":format!("合成{kind} {i:03} · 沿着海岸走到日落之后（特别收录版本）"),"ar":[{"name":format!("Artist {} · Cumulus", 150-i)}],"al":{"name":format!("专辑 {:03}", 150-i)},"dt":235000,"fee":0}),
                    );
                    if (121..=132).contains(&i) {
                        task.song.as_mut().unwrap()["voice_id"] = json!(i.to_string());
                    }
                    if voice_podcast_enabled() && (121..=123).contains(&i) {
                        let id = i + 580;
                        task.origin = DownloadOrigin::Voice {
                            voice_id: id.to_string(),
                        };
                        let raw = voice_episode(if i == 123 { id + 1 } else { id }, i);
                        let mut metadata = cumulus_core::podcast::Episode::from_official(&raw)?
                            .download_metadata()?;
                        if i == 122 {
                            metadata["voice"]["audio"]["id"] = Value::Null;
                            metadata["voice"]["audio"]["name"] = json!("没有编号的播客");
                        }
                        if i == 123 {
                            metadata["voice"]["audio"]["name"] = json!("错误的归属不能展示");
                        }
                        task.song = Some(metadata);
                    }
                    if i == 133 {
                        task.song.as_mut().unwrap()["dt"] = json!(6000);
                        task.song.as_mut().unwrap()["ar"] =
                            json!([{"id":17,"name":"Artist 17"},{"name":"无标识艺人"}]);
                    }
                    task.created_ms = 1700000000000 + i * 1000;
                    if lyric_repair && i == 1 {
                        // Create a real publication receipt and fail only the
                        // sidecar request. A hand-seeded Complete row cannot
                        // satisfy the repair worker's file identity checks.
                        self.store.put(
                            "2",
                            "download_tasks_v1",
                            &task.id,
                            &serde_json::to_value(&task)?,
                        )?;
                        let previous_chunk = CHUNK.swap(usize::MAX, Ordering::Relaxed);
                        SEED_LYRIC_FAILURE.store(true, Ordering::Relaxed);
                        let published = service.run(&task.id, DownloadRunOptions::default()).await;
                        SEED_LYRIC_FAILURE.store(false, Ordering::Relaxed);
                        CHUNK.store(previous_chunk, Ordering::Relaxed);
                        let mut task = published?;
                        task.completed_ms = Some(1700000000000 + (150 - i) * 1000);
                        let value = serde_json::to_value(&task)?;
                        assert_eq!(value["lyrics_status"], "error");
                        assert_eq!(value["cleanup_pending"], true);
                        self.store.put("2", "download_tasks_v1", &task.id, &value)?;
                        saved.push((task.id, value));
                        continue;
                    }
                    task.resource = Some(json!({"size":18_000_000,"level":"lossless","r":1080}));
                    task.bytes = 9_000_000;
                    if i <= 141 {
                        // Only 133 is a real video; other MV cards are layout fixtures.
                        let integrity = if i == 133 {
                            std::fs::write(&path, include_bytes!("fixtures/offline-mv.mp4"))?;
                            cumulus_core::media::integrity::inspect_file(&path)?
                        } else {
                            // Independent downloads have independent identities;
                            // sharing every inode also aliases an unrelated
                            // paused decoder on Windows. Explicit alias/conflict
                            // fixtures below remain separate deliberate cases.
                            std::fs::copy(&wave, &path)?;
                            integrity.clone()
                        };
                        task.target = Some(path.clone());
                        task.output_integrity = Some(integrity.clone());
                        task.status = DownloadStatus::Complete;
                        task.phase = DownloadPhase::Complete;
                        task.completed_ms = Some(1700000000000 + (150 - i) * 1000);
                        if i <= 133 {
                            offline.register(
                                &token,
                                &cumulus_core::download::offline::PublishedDownload {
                                    task_id: task.id.clone(),
                                    song_id: task.song_id.clone(),
                                    requested_quality: task.options.quality,
                                    target: path,
                                    song: task.song.clone().unwrap(),
                                    resource: task.resource.clone().unwrap(),
                                    encrypted: false,
                                    integrity: integrity.clone(),
                                },
                            )?;
                        }
                    } else {
                        task.phase = DownloadPhase::Authorize;
                        task.bytes = 0;
                        task.status = match i {
                            142 => DownloadStatus::Downloading,
                            143 => DownloadStatus::Paused,
                            144 => DownloadStatus::Error,
                            145 => DownloadStatus::Queued,
                            _ => DownloadStatus::Processing,
                        };
                        if i == 144 {
                            task.last_error = Some("synthetic destination exists".into());
                        }
                    }
                    let mut value = serde_json::to_value(&task)?;
                    if task.status == DownloadStatus::Complete {
                        // Simulate the target-parent receipt normally committed
                        // by the native authorize/publish pipeline.
                        value["target_parent"] = value["root_identity"].clone();
                    }
                    self.store.put("2", "download_tasks_v1", &task.id, &value)?;
                    saved.push((task.id, value));
                    if i % 25 == 0 || i == 146 {
                        eprintln!(
                            "download UI fixture seed: {i}/146 records in {:?}",
                            started.elapsed()
                        );
                    }
                }
                *self.records.lock().unwrap() = saved;
            }
            "playback_download_badge" => {
                let state = value["state"]
                    .as_str()
                    .filter(|value| matches!(*value, "queued" | "downloading" | "complete"))
                    .ok_or(Error::InvalidInput(
                        "invalid synthetic playback badge state",
                    ))?;
                let records = self.records.lock().unwrap();
                let (id, original) = records
                    .iter()
                    .find(|(_, row)| row["song_id"] == "10")
                    .ok_or(Error::State("missing synthetic playback badge song"))?;
                let mut row = original.clone();
                row["status"] = json!(state);
                self.store.put("2", "download_tasks_v1", id, &row)?;
            }
            "assert_lyric_repair_pending" | "verify_lyric_repair" => {
                let id = value["id"]
                    .as_str()
                    .ok_or(Error::InvalidInput("missing fixture task"))?;
                let seed = self
                    .records
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, record)| record.clone())
                    .ok_or(Error::InvalidInput("not a seeded fixture task"))?;
                let row = self
                    .store
                    .get("2", "download_tasks_v1", id)?
                    .ok_or(Error::InvalidInput("missing seeded fixture task"))?;
                if row["origin"] != seed["origin"] || row["status"] != "complete" {
                    return Err(Error::State("not a completed fixture task"));
                }
                let target = PathBuf::from(
                    seed["target"]
                        .as_str()
                        .ok_or(Error::State("missing seeded audio target"))?,
                );
                assert!(target.starts_with(self.root.join("media").canonicalize()?));
                if mode == "assert_lyric_repair_pending" {
                    assert!(!target.with_extension("lrc").exists());
                    assert_eq!(row["options"]["lyrics"], true);
                    assert_eq!(row["lyrics_status"], "error");
                    assert_eq!(row["cleanup_pending"], true);
                } else {
                    assert_eq!(row["lyrics_status"], "complete");
                    assert_eq!(row["cleanup_pending"], false);
                    assert_eq!(row["output_integrity"], seed["output_integrity"]);
                    assert_eq!(row["authorization_count"], seed["authorization_count"]);
                    assert_eq!(
                        serde_json::to_value(cumulus_core::media::integrity::inspect_file(
                            &target
                        )?)?,
                        seed["output_integrity"]
                    );
                    assert!(
                        std::fs::read_to_string(target.with_extension("lrc"))?.contains("合成歌词")
                    );
                    if let Some((_, record)) = self
                        .records
                        .lock()
                        .unwrap()
                        .iter_mut()
                        .find(|(key, _)| key == id)
                    {
                        *record = row;
                    }
                }
            }
            "read_error" => {
                self.store
                    .put("2", "download_tasks_v1", "broken-ui-fixture", &json!({}))?
            }
            "repair_read" => self
                .store
                .delete("2", "download_tasks_v1", "broken-ui-fixture")?,
            "empty" => {
                for (id, _) in self.records.lock().unwrap().iter() {
                    self.store.delete("2", "download_tasks_v1", id)?;
                }
            }
            "restore" => {
                for (id, record) in self.records.lock().unwrap().iter() {
                    self.store.put("2", "download_tasks_v1", id, record)?;
                }
            }
            "verify_pause" => {
                let mut before = vec![];
                for (id, _) in self.records.lock().unwrap().iter() {
                    let record = self.store.get("2", "download_tasks_v1", id)?.unwrap();
                    let song = record["song_id"].as_str().unwrap();
                    if ["142", "145", "146"].contains(&song) {
                        assert_eq!(record["status"], "paused");
                        assert!(record["bytes"].as_u64().unwrap() > 0);
                        before.push((id.clone(), record["bytes"].clone()));
                    }
                    if song == "144" {
                        assert_eq!(record["status"], "error");
                    }
                }
                assert_eq!(before.len(), 3);
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                for (id, bytes) in before {
                    assert_eq!(
                        self.store.get("2", "download_tasks_v1", &id)?.unwrap()["bytes"],
                        bytes
                    );
                }
            }
            "progress" => {
                CHUNK.store(8192, Ordering::Relaxed);
            }
            "verify_queue" => {
                let expected: Vec<String> = value["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_u64().unwrap().to_string())
                    .collect();
                let current_id = value["current"].as_u64().unwrap().to_string();
                let player = client.subscribe_playback(&epoch).await?;
                tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    loop {
                        let queue = client.player_queue(&epoch).await?;
                        let ids: Vec<_> = queue
                            .tracks
                            .iter()
                            .map(|item| item.track.id.clone())
                            .collect();
                        let current = queue
                            .tracks
                            .iter()
                            .find(|item| Some(&item.entry_id) == queue.current.as_ref());
                        let state = player.current()?;
                        if ids == expected
                            && current.is_some_and(|item| item.track.id == current_id)
                            && state
                                .track
                                .as_ref()
                                .is_some_and(|track| track.id == current_id)
                            && state.state
                                == cumulus_core::playback::controller::PlaybackStatus::Playing
                            && state.position.is_some_and(|p| p > 0.05)
                        {
                            return Ok::<_, Error>(());
                        }
                        if state.error.is_some() {
                            return Err(Error::State("synthetic download playback failed"));
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                })
                .await
                .map_err(|_| Error::State("synthetic download playback timed out"))??;
            }
            "verify_local_mv" => {
                let current = client.video_snapshot(&epoch).await?;
                if current.id.as_deref() != Some("133")
                    || !current.local
                    || current.requested_resolution != 1080
                    || current.actual_resolution != Some(1080)
                    || MV_STREAM.load(Ordering::SeqCst) != 0
                    || MV_DETAIL.load(Ordering::SeqCst) == 0
                {
                    return Err(Error::State(
                        "real local video did not load independently of online metadata",
                    ));
                }
            }
            "verify_batch_delete" => {
                let removed = value["removed"]
                    .as_u64()
                    .ok_or(Error::InvalidInput("missing synthetic file id"))?;
                let retained = value["retained"]
                    .as_u64()
                    .ok_or(Error::InvalidInput("missing synthetic file id"))?;
                if !(1..=132).contains(&removed)
                    || !(1..=132).contains(&retained)
                    || removed == retained
                {
                    return Err(Error::InvalidInput("invalid synthetic file ids"));
                }
                if self
                    .root
                    .join("media")
                    .join(format!("synthetic-{removed}.wav"))
                    .exists()
                    || !self
                        .root
                        .join("media")
                        .join(format!("synthetic-{retained}.wav"))
                        .is_file()
                {
                    return Err(Error::State("unexpected batch file deletion outcome"));
                }
            }
            "remove_first" => {
                let records = self.records.lock().unwrap();
                self.store.delete("2", "download_tasks_v1", &records[0].0)?;
            }
            "menu_targets" => {
                let media = self.root.join("media");
                std::fs::create_dir_all(media.join("menu-moved"))?;
                std::fs::hard_link(media.join("synthetic-1.wav"), media.join("ui-conflict.wav"))?;
            }
            "verify_menu_files" => {
                let media = self.root.join("media");
                assert!(media.join("menu-moved/menu-renamed.wav").is_file());
                assert!(!media.join("synthetic-1.wav").exists());
                assert!(!media.join("menu-renamed.wav").exists());
                assert!(media.join("ui-conflict.wav").is_file());
                assert!(media.join("synthetic-2.wav").is_file());
                assert!(!media.join("synthetic-3.wav").exists());
                assert!(media.join("synthetic-4.wav").is_file());
                let records = self.records.lock().unwrap();
                assert!(
                    self.store
                        .values("2", "download_file_operations_v1")?
                        .values()
                        .any(|operation| operation["task_id"] == records[2].0
                            && operation["action"] == "delete"
                            && operation["phase"] == "complete")
                );
                for (index, (id, _)) in records.iter().take(3).enumerate() {
                    let record = self.store.get("2", "download_tasks_v1", id)?;
                    if index == 0 {
                        assert_eq!(
                            std::fs::canonicalize(record.unwrap()["target"].as_str().unwrap())?,
                            std::fs::canonicalize(media.join("menu-moved/menu-renamed.wav"))?
                        );
                    } else {
                        assert!(record.is_none());
                    }
                }
            }
            "verify_menu_queue" => {
                let queue: cumulus_core::playback::QueueSnapshot = serde_json::from_value(
                    self.store
                        .get("2", "playback", "queue")?
                        .ok_or(Error::State("missing synthetic queue"))?,
                )?;
                let records = self.records.lock().unwrap();
                assert_eq!(queue.items[1].resource["download_task_id"], records[1].0);
                assert_eq!(queue.items[1].source["kind"], "downloads");
            }
            "verify_single_cancel" => {
                let records = self.records.lock().unwrap();
                for (index, (id, _)) in records.iter().enumerate().skip(141) {
                    let record = self
                        .store
                        .get("2", "download_tasks_v1", id)?
                        .ok_or(Error::State("missing synthetic task"))?;
                    if index == 142 {
                        assert_eq!(record["status"], "cancelled");
                    } else {
                        assert_ne!(record["status"], "cancelled");
                    }
                }
            }
            "mv_window" => {
                // Reclassify existing synthetic records for a >100-card window;
                // these PCM hardlinks remain layout fixtures, never playable MV.
                for (id, saved) in self.records.lock().unwrap().iter().take(120) {
                    let mut record = saved.clone();
                    record["origin"] =
                        serde_json::to_value(DownloadOrigin::Mv { resolution: 1080 })?;
                    record["song"]["name"] = json!(format!("合成MV扩展 {}", record["song_id"]));
                    self.store.put("2", "download_tasks_v1", id, &record)?;
                }
            }
            _ => return Err(Error::InvalidInput("unknown download UI fixture mode")),
        }
        eprintln!(
            "download UI fixture {mode}: completed in {:?}",
            started.elapsed()
        );
        Ok(())
    }
}
