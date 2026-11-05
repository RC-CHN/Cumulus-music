//! One native synthetic publication and a real decoder boundary for a Vue search row.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    client::{MusicClient, dto::DownloadCommand},
    download::{
        offline::OfflineRecord,
        service::{DownloadOptions, DownloadOrigin, DownloadStatus, DownloadTask, OutputContainer},
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    media::integrity::inspect_file,
    membership::Quality,
    playback::{controller::PlaybackStatus, engine::*},
    protocol::api::ApiFuture,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const ID: &str = "881";
const OWNER: &str = "2";
const QUERY: &str = "合成下载优先";
const DOWNLOAD: &str = "https://m1.music.126.net/search-download-proof.flac";
const ONLINE: &str = "https://m1.music.126.net/cumulus-relay-fixture.wav";
const SEARCH_PATH: &str = "/api/search/pc/complex/page";
const DOWNLOAD_PATH: &str = "/api/song/enhance/download/url/v1";
const SOURCE_PATH: &str = "/api/song/enhance/player/url/v1";
const SEED: &[u8] = include_bytes!("../../../core/tests/fixtures/stereo-pcm-source.flac");
const MD5: &str = "4e25192f88135eb81a995c6aa5fa7848";
const TASKS: &str = "download_tasks_v1";
const LEDGER: &str = "offline_music_v1";

struct Publication {
    path: PathBuf,
    audit: Value,
    corrupted: bool,
}
struct Trace {
    search: Vec<Value>,
    download: Vec<Value>,
    online: Vec<Value>,
    loads: Vec<Value>,
    media_reads: u32,
    publication: Option<Publication>,
}
static TRACE: Mutex<Trace> = Mutex::new(Trace {
    search: Vec::new(),
    download: Vec::new(),
    online: Vec::new(),
    loads: Vec::new(),
    media_reads: 0,
    publication: None,
});

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_DOWNLOAD_ROW_PROOF").is_some()
}
fn song() -> Value {
    json!({"id":881,"name":"合成下载歌曲","dt":2000,"fee":0,"ar":[{"name":"合成歌手"}],"al":{"name":"合成专辑"}})
}
fn privilege() -> Value {
    json!({"id":881,"fee":0,"st":0,"dl":999000,"pl":320000,"maxbr":999000,"flag":0})
}
pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let denied = || Error::InvalidInput("unexpected search-download proof request");
    let mut trace = TRACE.lock().unwrap();
    Some(match path {
        SEARCH_PATH => {
            trace.search.push(args.clone());
            if cached
                || trace.search.len() != 1
                || *args
                    != json!({"keyword":QUERY,"scene":"normal",
                "needCorrect":true,"channel":"typing","cursor":"{\"page\":0,\"ignoreBlocks\":[],\"traceId\":\"\"}","bizQueryInfo":""})
            {
                return Some(Err(denied()));
            }
            Ok(
                json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_song","resources":[
                    {"type":"song","baseInfo":song(),"extInfo":{"privilege":privilege()}}
                ]}]}}),
            )
        }
        "/api/v3/song/detail"
            if args["c"]
                .as_str()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                .and_then(|rows| rows.as_array().cloned())
                .is_some_and(|rows| {
                    rows.len() == 1 && (rows[0]["id"] == ID || rows[0]["id"] == 881)
                }) =>
        {
            Ok(json!({"code":200,"songs":[song()],"privileges":[privilege()]}))
        }
        DOWNLOAD_PATH => {
            trace.download.push(args.clone());
            if cached
                || trace.download.len() != 1
                || *args != json!({"id":ID,"level":"lossless","immerseType":"c51"})
            {
                return Some(Err(denied()));
            }
            Ok(
                json!({"code":200,"data":{"id":881,"code":200,"url":DOWNLOAD,"type":"flac",
                "size":SEED.len(),"md5":MD5,"level":"lossless"}}),
            )
        }
        SOURCE_PATH => {
            trace.online.push(args.clone());
            if cached
                || trace.online.len() != 1
                || !trace
                    .publication
                    .as_ref()
                    .is_some_and(|value| value.corrupted)
                || *args
                    != json!({"ids":"[\"881\"]","level":"standard","encodeType":"aac","immerseType":"c51","trialMode":42})
            {
                return Some(Err(denied()));
            }
            Ok(
                json!({"code":200,"data":[{"id":881,"code":200,"url":ONLINE,"level":"standard","md5":"fixture","expi":120}]}),
            )
        }
        _ => return None,
    })
}

pub struct Media;
struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let n = self.0.len().min(output.len());
            output[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        })
    }
}
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let mut trace = TRACE.lock().unwrap();
            trace.media_reads += 1;
            if trace.media_reads != 1
                || request.url.as_str() != DOWNLOAD
                || request.offset != 0
                || request.if_range.is_some()
            {
                return Err(Error::InvalidInput(
                    "unexpected search-download proof transfer",
                ));
            }
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), SEED.len().to_string())]),
                body: Box::new(Body(SEED)),
            })
        })
    }
}

pub async fn control(
    client: &MusicClient,
    session: &AccountSession,
    store: &StateStore,
    root: &Path,
    mode: &str,
) -> Result<()> {
    if !enabled()
        || !store.directory().starts_with(root)
        || session.token()?.uid.as_deref() != Some(OWNER)
    {
        return Err(Error::InvalidInput(
            "search-download proof requires the isolated account",
        ));
    }
    let epoch = client.account()?.epoch;
    match mode {
        "publish" => {
            if !store.values(OWNER, TASKS)?.is_empty() || !store.values(OWNER, LEDGER)?.is_empty() {
                return Err(Error::State(
                    "search-download proof requires an empty publication store",
                ));
            }
            let media = root.join("search-download-media");
            fs::create_dir(&media)?;
            let media = media.canonicalize()?;
            let setup = client
                .download_command(
                    &epoch,
                    DownloadCommand::Configure {
                        root: media
                            .to_str()
                            .ok_or(Error::InvalidInput("non-UTF8 proof path"))?
                            .into(),
                        path_type: 0,
                        file_type: 1,
                    },
                )
                .await?;
            if !setup.failed.is_empty() {
                return Err(Error::State("search-download proof configure failed"));
            }
            let output = client
                .download_command(
                    &epoch,
                    DownloadCommand::Start {
                        song_ids: vec![ID.into()],
                        options: DownloadOptions {
                            quality: Quality::Lossless,
                            tags: false,
                            cover: false,
                            lyrics: false,
                            container: OutputContainer::Official,
                        },
                    },
                )
                .await?;
            let [task_id] = output.accepted.as_slice() else {
                return Err(Error::State("search-download proof task not accepted"));
            };
            if !output.failed.is_empty() {
                return Err(Error::State("search-download proof task failed"));
            }
            let (task, raw) = tokio::time::timeout(Duration::from_secs(18), async {
                loop {
                    let raw = store
                        .get(OWNER, TASKS, task_id)?
                        .ok_or(Error::State("search-download proof task missing"))?;
                    let task: DownloadTask = serde_json::from_value(raw.clone())?;
                    match task.status {
                        DownloadStatus::Complete if !task.cleanup_pending => {
                            return Ok((task, raw));
                        }
                        DownloadStatus::Error
                        | DownloadStatus::Cancelled
                        | DownloadStatus::Paused => {
                            return Err(Error::State("search-download proof worker failed"));
                        }
                        _ => tokio::time::sleep(Duration::from_millis(65)).await,
                    }
                }
            })
            .await
            .map_err(|_| Error::State("search-download proof publication timed out"))??;
            let path = task
                .target
                .as_ref()
                .ok_or(Error::State("search-download proof target missing"))?;
            if path.parent() != Some(media.as_path())
                || !fs::symlink_metadata(path)?.file_type().is_file()
                || fs::read(path)? != SEED
            {
                return Err(Error::State(
                    "search-download proof target is not the exact frozen FLAC",
                ));
            }
            let integrity = inspect_file(path)?;
            let records = store.values(OWNER, LEDGER)?;
            let (_, record) = records
                .first_key_value()
                .ok_or(Error::State("search-download proof ledger missing"))?;
            let record: OfflineRecord = serde_json::from_value(record.clone())?;
            if records.len() != 1
                || record.path != *path
                || record.task_id != *task_id
                || record.song_id != ID
                || record.encrypted
                || record.quality != Quality::Lossless
                || record.integrity != integrity
                || task.song_id != ID
                || !matches!(task.origin, DownloadOrigin::Song)
                || task.encrypted
                || task.authorization_count != 1
                || task.output_integrity.as_ref() != Some(&integrity)
                || task.raw_integrity.as_ref().is_none_or(|value| {
                    value.md5 != MD5
                        || value.sha256 != integrity.sha256
                        || value.bytes != integrity.bytes
                })
                || raw["target_parent"].is_null()
            {
                return Err(Error::State(
                    "search-download proof native publication mismatch",
                ));
            }
            TRACE.lock().unwrap().publication = Some(Publication {
                path: path.clone(),
                corrupted: false,
                audit: json!({"verified":true,"bytes":integrity.bytes,"sha256":integrity.sha256,"taskId":task_id}),
            });
            Ok(())
        }
        "corrupt" => {
            let player = client.player_snapshot(&epoch).await?;
            let mut trace = TRACE.lock().unwrap();
            if player.state != PlaybackStatus::Idle
                || trace.loads.len() != 1
                || !trace.online.is_empty()
            {
                return Err(Error::State(
                    "search-download proof must stop local playback before tampering",
                ));
            }
            let publication = trace.publication.as_mut().ok_or(Error::State(
                "search-download proof publication not prepared",
            ))?;
            if publication.corrupted
                || publication.path.parent()
                    != Some(root.join("search-download-media").canonicalize()?.as_path())
                || !fs::symlink_metadata(&publication.path)?
                    .file_type()
                    .is_file()
                || fs::read(&publication.path)? != SEED
            {
                return Err(Error::State("search-download proof tamper scope mismatch"));
            }
            fs::write(&publication.path, vec![0; SEED.len()])?;
            publication.corrupted = true;
            Ok(())
        }
        _ => Err(Error::InvalidInput("unknown search-download proof control")),
    }
}

pub fn report() -> Value {
    let trace = TRACE.lock().unwrap();
    json!({"search":trace.search,"download":trace.download,"online":trace.online,"loads":trace.loads,
        "mediaReads":trace.media_reads,"publication":trace.publication.as_ref().map(|value| &value.audit),
        "corrupted":trace.publication.as_ref().is_some_and(|value| value.corrupted)})
}

pub struct Audio {
    inner: super::relay_fixture::NativeAudio,
}
impl Audio {
    pub fn new(inner: super::relay_fixture::NativeAudio) -> Self {
        Self { inner }
    }
}
impl AudioEngine for Audio {
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let (kind, index) = {
            let mut trace = TRACE.lock().unwrap();
            let kind = match &request.source {
                MediaInput::File(path)
                    if trace
                        .publication
                        .as_ref()
                        .is_some_and(|record| !record.corrupted && record.path == *path) =>
                {
                    "published_flac"
                }
                MediaInput::Url(url) if url.as_str() == ONLINE && trace.online.len() == 1 => {
                    "authorized_url"
                }
                _ => "unexpected",
            };
            let index = trace.loads.len();
            trace
                .loads
                .push(json!({"kind":kind,"start":request.start,"end":request.end}));
            (kind, index)
        };
        if !matches!((kind, index), ("published_flac", 0) | ("authorized_url", 1)) {
            return Err(Error::InvalidInput(
                "unexpected search-download proof audio input",
            ));
        }
        let loaded = self.inner.load(request, cancel)?;
        let mut trace = TRACE.lock().unwrap();
        trace.loads[index]["codec"] = json!(loaded.codec);
        trace.loads[index]["nativeEntry"] = json!(loaded.entry_id);
        trace.loads[index]["duration"] = json!(loaded.duration);
        Ok(loaded)
    }
    fn pause(&mut self, value: bool) -> Result<()> {
        self.inner.pause(value)
    }
    fn seek(&mut self, value: f64) -> Result<()> {
        self.inner.seek(value)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.inner.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.inner.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.inner.poll()
    }
}
