//! Observe real libmpv EOF events without changing what the production actor receives.
use super::relay_fixture;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::{
        offline::OfflineLibrary,
        service::{
            DownloadDestination, DownloadOptions, DownloadOrigin, DownloadRunOptions,
            DownloadService, DownloadStatus,
        },
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    media::integrity::inspect_file,
    membership::Quality,
    playback::{
        PlaybackMode,
        engine::{
            AudioEngine, Cancellation, EndReason, EngineEvent, EngineSnapshot, LoadRequest,
            LoadedMedia, MediaInput, MpvConfig, MpvEngine,
        },
    },
    protocol::api::{Api, ApiFuture, identifier},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub type Trace = Arc<Mutex<Vec<Value>>>;
pub const FIRST_SECONDS: u32 = 8;
pub const TARGET_SECONDS: u32 = 30;
pub const DOWNLOAD_SECONDS: u32 = 8;
const FIRST: &str = "https://m1.music.126.net/cumulus-eof-fixture-first.wav";
const TARGET: &str = "https://m1.music.126.net/cumulus-eof-fixture-target.wav";
const DOWNLOAD: &str = "https://m1.music.126.net/cumulus-eof-fixture-publication.flac";
const FLAC: &[u8] =
    include_bytes!("../../../core/tests/fixtures/held_audio/search-playback-8s.flac");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Skip,
    Control,
    DeniedTail,
    OrderLoopSkip,
    RandomSkip,
    RandomControl,
    Download,
    DownloadStale,
}

impl Case {
    pub fn from_env() -> Option<Self> {
        match std::env::var("CUMULUS_ORDINARY_EOF_CASE").as_deref() {
            Ok("t1-skip") => Some(Self::Skip),
            Ok("control") => Some(Self::Control),
            Ok("t1-tail") => Some(Self::DeniedTail),
            Ok("order-loop-skip") => Some(Self::OrderLoopSkip),
            Ok("random-skip") => Some(Self::RandomSkip),
            Ok("random-control") => Some(Self::RandomControl),
            Ok("t1-download") => Some(Self::Download),
            Ok("t1-download-stale") => Some(Self::DownloadStale),
            Ok(_) => panic!("unknown sealed EOF case"),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => panic!("invalid EOF case: {error}"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Skip => "t1-skip",
            Self::Control => "control",
            Self::DeniedTail => "t1-tail",
            Self::OrderLoopSkip => "order-loop-skip",
            Self::RandomSkip => "random-skip",
            Self::RandomControl => "random-control",
            Self::Download => "t1-download",
            Self::DownloadStale => "t1-download-stale",
        }
    }

    pub fn group(self) -> &'static str {
        if matches!(self, Self::Control | Self::RandomControl) {
            "c"
        } else {
            "t1"
        }
    }

    pub fn mode(self) -> PlaybackMode {
        match self {
            Self::OrderLoopSkip => PlaybackMode::OrderLoop,
            Self::RandomSkip | Self::RandomControl => PlaybackMode::Random,
            _ => PlaybackMode::Sequential,
        }
    }

    pub fn displayed_rows(self) -> [u64; 3] {
        match self {
            Self::OrderLoopSkip => [2, 3, 1],
            Self::RandomSkip => [1, 3, 2],
            _ => [1, 2, 3],
        }
    }

    pub fn random_deck(self) -> Option<[usize; 3]> {
        // Both random cases use [3,1,2]. In treatment, the different display
        // order would authorize 3 directly. In control, 2 is denied in both
        // the random deck and the sequential display order after mode change.
        match self {
            Self::RandomSkip => Some([1, 0, 2]),
            Self::RandomControl => Some([2, 0, 1]),
            _ => None,
        }
    }

    pub fn playable(self, id: u64) -> bool {
        id == 1 || id == 3 && self != Self::DeniedTail
    }

    pub fn download(self) -> bool {
        matches!(self, Self::Download | Self::DownloadStale)
    }

    pub fn url(self, id: &str) -> Option<&'static str> {
        match id {
            "1" => Some(FIRST),
            "3" if self != Self::DeniedTail && !self.download() => Some(TARGET),
            _ => None,
        }
    }
}

pub async fn publish(root: &Path, store: Arc<StateStore>, stale: bool) -> Result<Value> {
    let directory = root.join("published-eof");
    fs::create_dir(&directory)?;
    let path = directory.join("song-3.flac");
    let api = Arc::new(PublicationApi);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.adopt(api, json!({"userId":2}), &session.token()?)?;
    let service =
        DownloadService::with_backend(session.clone(), Arc::new(PublicationMedia), 256 * 1024)?;
    let task = service.create(
        &json!("3"),
        DownloadDestination::Path { path: path.clone() },
        DownloadOptions {
            quality: Quality::Lossless,
            tags: false,
            cover: false,
            lyrics: false,
            ..Default::default()
        },
    )?;
    let task = service.run(&task.id, DownloadRunOptions::default()).await?;
    let path = path.canonicalize()?;
    assert_eq!(task.status, DownloadStatus::Complete);
    assert_eq!(task.origin, DownloadOrigin::Song);
    assert_eq!(task.song_id, "3");
    assert_eq!(task.target.as_deref(), Some(path.as_path()));
    assert_eq!(task.authorization_count, 1);
    assert_eq!(fs::read(&path)?.as_slice(), FLAC);
    let record = OfflineLibrary::new(session.clone())?
        .record(&session.token()?, &path)?
        .ok_or(Error::State("EOF publication missing from owner's ledger"))?;
    assert_eq!(record.task_id, task.id);
    assert_eq!(record.song_id, "3");
    assert_eq!(task.output_integrity.as_ref(), Some(&record.integrity));
    assert!(!record.encrypted);
    if stale {
        let mut changed = FLAC.to_vec();
        changed[FLAC.len() - 16] ^= 1;
        fs::write(&path, changed)?;
    }
    let actual = inspect_file(&path)?;
    assert_eq!(actual.bytes, record.integrity.bytes);
    assert_eq!(actual == record.integrity, !stale);
    Ok(json!({
        "owner":"2","songId":record.song_id,"taskId":task.id,"bytes":record.integrity.bytes,
        "sha256":record.integrity.sha256,"fileSha256":actual.sha256,"stale":stale,"intact":!stale,
        "authorizationCount":task.authorization_count,
    }))
}

struct PublicationApi;
impl Api for PublicationApi {
    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cached);
            Ok(match path {
                "/api/song/enhance/download/url/v1" => {
                    assert_eq!(identifier(&args["id"])?, "3");
                    assert_eq!(args["level"], "lossless");
                    json!({"code":200,"data":{"id":3,"code":200,"url":DOWNLOAD,"size":FLAC.len(),
                           "type":"flac","level":"lossless","flag":0,"payed":0}})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> = serde_json::from_str(args["c"].as_str().unwrap())?;
                    assert_eq!(ids.len(), 1);
                    assert_eq!(identifier(&ids[0]["id"])?, "3");
                    json!({"code":200,"songs":[{"id":3,"name":"合成已下载歌曲","dt":DOWNLOAD_SECONDS * 1000,
                           "ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},"fee":0}],
                           "privileges":[{"id":3,"fee":0,"st":0,"pl":128000,"dl":128000}]})
                }
                _ => return Err(Error::Response("unexpected sealed EOF publication request")),
            })
        })
    }
}

struct PublicationMedia;
impl MediaBackend for PublicationMedia {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(request.url.as_str(), DOWNLOAD);
            assert_eq!(request.offset, 0);
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([
                    ("content-length".into(), FLAC.len().to_string()),
                    ("etag".into(), "\"cumulus-eof-publication\"".into()),
                ]),
                body: Box::new(PublicationBytes(FLAC)),
            })
        })
    }
}

struct PublicationBytes(&'static [u8]);
impl MediaBody for PublicationBytes {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(self.0.read(buffer)?) })
    }
}

#[derive(Clone)]
pub struct Media {
    first: PathBuf,
    target: PathBuf,
}
impl Media {
    pub fn generate(root: &Path) -> Result<Self> {
        let first = root.join("eof-first");
        let target = root.join("eof-target");
        std::fs::create_dir(&first)?;
        std::fs::create_dir(&target)?;
        Ok(Self {
            first: relay_fixture::wave_seconds(&first, FIRST_SECONDS)?,
            target: relay_fixture::wave_seconds(&target, TARGET_SECONDS)?,
        })
    }
}

pub struct ObservedAudio {
    engine: MpvEngine,
    media: Media,
    trace: Trace,
    store: Arc<StateStore>,
    entry: Option<i64>,
    output_entry: Option<i64>,
    download: bool,
}
impl ObservedAudio {
    pub fn open(
        media: Media,
        trace: Trace,
        store: Arc<StateStore>,
        download: bool,
    ) -> Result<Self> {
        Ok(Self {
            engine: MpvEngine::open(MpvConfig {
                null_output: true,
                ..Default::default()
            })?,
            media,
            trace,
            store,
            entry: None,
            output_entry: None,
            download,
        })
    }
}
impl AudioEngine for ObservedAudio {
    fn load(&mut self, mut request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let (id, path) = match &request.source {
            MediaInput::Url(url) if url.as_str() == FIRST => ("1", &self.media.first),
            MediaInput::Url(url) if !self.download && url.as_str() == TARGET => {
                ("3", &self.media.target)
            }
            _ => return Err(Error::InvalidInput("EOF proof refused non-fixture media")),
        };
        request.source = MediaInput::File(path.clone());
        let checkpoint = self.store.get("2", "playback", "position")?;
        let receipt = self.engine.load(request, cancel)?;
        self.entry = Some(receipt.entry_id);
        self.trace.lock().unwrap().push(json!({
            "kind":"native_load","id":id,"entry":receipt.entry_id,"duration":receipt.duration,
            "positionBeforeLoad":checkpoint
        }));
        Ok(receipt)
    }

    fn supports_pinned_search_download(&self) -> bool {
        self.download && self.engine.supports_pinned_search_download()
    }

    fn load_pinned_audio(
        &mut self,
        mut file: File,
        authority: Cancellation,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        if !self.download || file.metadata()?.len() != FLAC.len() as u64 {
            return Err(Error::InvalidInput(
                "EOF proof refused an unexpected held file",
            ));
        }
        authority.check()?;
        cancel.check()?;
        file.rewind()?;
        let mut bytes = vec![0; FLAC.len()];
        file.read_exact(&mut bytes)?;
        if bytes != FLAC {
            return Err(Error::Media("EOF proof received changed FLAC bytes"));
        }
        file.rewind()?;
        let checkpoint = self.store.get("2", "playback", "position")?;
        let receipt = self.engine.load_pinned_audio(file, authority, cancel)?;
        self.entry = Some(receipt.entry_id);
        self.trace.lock().unwrap().push(json!({
            "kind":"native_load","id":"3","entry":receipt.entry_id,"duration":receipt.duration,
            "positionBeforeLoad":checkpoint,"source":"held_flac","codec":receipt.codec,
            "verifiedFixtureBytes":true,
        }));
        Ok(receipt)
    }

    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused)
    }
    fn seek(&mut self, _: f64) -> Result<()> {
        Err(Error::State("EOF proof cannot seek"))
    }
    fn volume(&mut self, volume: f64) -> Result<()> {
        self.engine.volume(volume)
    }
    fn speed(&mut self, speed: f64) -> Result<()> {
        self.engine.speed(speed)
    }

    fn stop(&mut self) -> Result<()> {
        self.engine.stop()?;
        if let Some(entry) = self.entry.take() {
            self.trace
                .lock()
                .unwrap()
                .push(json!({"kind":"native_stop","entry":entry}));
        }
        Ok(())
    }

    fn poll(&mut self) -> Result<EngineSnapshot> {
        let snapshot = self.engine.poll()?;
        if let Some(entry) = snapshot.entry_id
            && self.entry == Some(entry)
            && self.output_entry != Some(entry)
            && let Some(output) = &snapshot.audio_output
        {
            self.output_entry = Some(entry);
            self.trace.lock().unwrap().push(json!({
                "kind":"native_output","entry":entry,"output":output
            }));
        }
        for event in &snapshot.events {
            if let EngineEvent::Ended {
                entry_id,
                reason: EndReason::Eof,
                error,
            } = event
            {
                self.trace.lock().unwrap().push(json!({
                    "kind":"native_eof","entry":entry_id,"error":error
                }));
            }
        }
        Ok(snapshot)
    }
}
