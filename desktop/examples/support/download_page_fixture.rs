//! Isolated native download records for UI inspection. No remote media transfer.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    client::{MusicClient, dto::DownloadCommand},
    download::{
        service::{
            DownloadDestination, DownloadOptions, DownloadOrigin, DownloadPhase, DownloadService,
            DownloadStatus,
        },
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    protocol::api::ApiFuture,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_DOWNLOAD_PAGE_UI_PROOF").is_some()
}
static DATA: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
static CHUNK: AtomicUsize = AtomicUsize::new(512);
static MV_DETAIL: AtomicUsize = AtomicUsize::new(0);
static MV_STREAM: AtomicUsize = AtomicUsize::new(0);
const MEDIA: &str = "https://m1.music.126.net/cumulus-download-fixture.wav";
pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
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
    if !enabled() || path != "/api/song/enhance/download/url/v1" {
        return None;
    }
    let data = DATA.get().expect("seed first");
    Some(Ok(
        json!({"code":200,"data":{"id":args["id"],"code":200,"url":MEDIA,"type":"wav","size":data.len(),"level":"standard"}}),
    ))
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
                            lyrics: false,
                            ..Default::default()
                        },
                    )?;
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
                    if i == 133 {
                        task.song.as_mut().unwrap()["dt"] = json!(6000);
                        task.song.as_mut().unwrap()["ar"] =
                            json!([{"id":17,"name":"Artist 17"},{"name":"无标识艺人"}]);
                    }
                    task.created_ms = 1700000000000 + i * 1000;
                    task.resource = Some(json!({"size":18_000_000,"level":"lossless","r":1080}));
                    task.bytes = 9_000_000;
                    if i <= 141 {
                        // Only 133 is a real video; other MV cards are layout fixtures.
                        let integrity = if i == 133 {
                            std::fs::write(&path, include_bytes!("fixtures/offline-mv.mp4"))?;
                            cumulus_core::media::integrity::inspect_file(&path)?
                        } else {
                            std::fs::hard_link(&wave, &path)?;
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
                    let value = serde_json::to_value(&task)?;
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
