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
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
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
    seeks: Vec<Value>,
    ends: Vec<Value>,
    media_reads: u32,
    publication: Option<Publication>,
}
static TRACE: Mutex<Trace> = Mutex::new(Trace {
    search: Vec::new(),
    download: Vec::new(),
    online: Vec::new(),
    loads: Vec::new(),
    seeks: Vec::new(),
    ends: Vec::new(),
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
        "seeks":trace.seeks,"ends":trace.ends,
        "mediaReads":trace.media_reads,"publication":trace.publication.as_ref().map(|value| &value.audit),
        "corrupted":trace.publication.as_ref().is_some_and(|value| value.corrupted)})
}

/// Isolated optional namespace change after native dispatch owns the File.
/// Keep the original descriptor alive and restore the test path before the
/// separate initial-rejection/online-fallback case.
struct PathReplacement<'a> {
    path: &'a Path,
    moved: PathBuf,
    active: bool,
    installed: bool,
}
impl<'a> PathReplacement<'a> {
    fn install(path: &'a Path) -> Result<Self> {
        let moved = path.with_extension("held-original");
        if fs::symlink_metadata(&moved).is_ok() || fs::read(path)? != SEED {
            return Err(Error::State("search proof rename precondition mismatch"));
        }
        fs::rename(path, &moved)?;
        let mut change = Self {
            path,
            moved,
            active: true,
            installed: false,
        };
        let mut replacement = OpenOptions::new().write(true).create_new(true).open(path)?;
        change.installed = true;
        replacement.write_all(&vec![0; SEED.len()])?;
        Ok(change)
    }

    fn restore(&mut self) -> Result<()> {
        if self.active {
            if self.installed {
                fs::remove_file(self.path)?;
                self.installed = false;
            }
            match fs::symlink_metadata(self.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => {
                    return Err(Error::State(
                        "search proof restore path is already occupied",
                    ));
                }
            }
            fs::rename(&self.moved, self.path)?;
            self.active = false;
        }
        Ok(())
    }
}
impl Drop for PathReplacement<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub struct Audio {
    inner: super::relay_fixture::NativeAudio,
}
impl Audio {
    pub fn new(mut inner: super::relay_fixture::NativeAudio) -> Result<Self> {
        if let Some(path) = std::env::var_os("CUMULUS_SEARCH_DOWNLOAD_EQ_LIBRARY") {
            let digest = std::env::var("CUMULUS_SEARCH_DOWNLOAD_EQ_SHA256")
                .map_err(|_| Error::InvalidInput("missing sealed FLAC digest"))?;
            inner = inner.with_search_graph(
                cumulus_core::playback::engine::TrustedFlacRuntime::new(path.into(), &digest)?,
            );
        }
        Ok(Self { inner })
    }

    fn load_verified(
        &mut self,
        mut file: File,
        authority: Cancellation,
        settings: Option<cumulus_core::effects::equalizer::EqualizerSettings>,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        authority.check()?;
        cancel.check()?;
        let path = {
            let trace = TRACE.lock().unwrap();
            if !trace.online.is_empty() || !trace.loads.is_empty() {
                return Err(Error::State("unexpected pinned search load"));
            }
            trace
                .publication
                .as_ref()
                .filter(|value| !value.corrupted)
                .ok_or(Error::State(
                    "search proof publication unavailable at native handoff",
                ))?
                .path
                .clone()
        };
        let replace = std::env::var_os("CUMULUS_SEARCH_DOWNLOAD_HELD_REPLACEMENT").is_some();
        let mut replacement = if replace {
            Some(PathReplacement::install(&path)?)
        } else {
            None
        };
        let mut bytes = Vec::new();
        (&mut file)
            .take(SEED.len() as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes != SEED {
            return Err(Error::State(
                "native search descriptor differs from frozen publication",
            ));
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        file.seek(SeekFrom::Start(0))?;
        let result = self
            .inner
            .load_verified_search_file(file, authority, settings, cancel);
        let still_replaced = !replace || fs::read(&path)? == vec![0; SEED.len()];
        if let Some(change) = &mut replacement {
            change.restore()?;
        }
        let loaded = result?;
        let graph = self.inner.search_engine().local_equalizer_entry() == Some(loaded.entry_id);
        if !still_replaced || fs::read(&path)? != SEED {
            return Err(Error::State(
                "search proof replacement was not isolated to native load",
            ));
        }
        TRACE.lock().unwrap().loads.push(json!({
            "kind":"published_flac", "start":0, "end":null,
            "transport":if graph {"held_equalizer_pcm"} else {"pinned_file"},
            "graphRequested":settings.is_some(),
            "handleSha256":hash, "handleBytes":bytes.len(), "heldReplacementVerified":replace,
            "codec":loaded.codec, "nativeEntry":loaded.entry_id, "duration":loaded.duration,
        }));
        Ok(loaded)
    }
}

impl AudioEngine for Audio {
    fn supports_pinned_search_download(&self) -> bool {
        true
    }

    fn load_pinned_audio(
        &mut self,
        file: File,
        authority: Cancellation,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        self.load_verified(file, authority, None, cancel)
    }

    fn load_pinned_audio_with_local_equalizer(
        &mut self,
        file: File,
        authority: Cancellation,
        settings: cumulus_core::effects::equalizer::EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        self.load_verified(file, authority, Some(settings), cancel)
    }

    fn supports_local_equalizer(&self) -> bool {
        self.inner.search_engine().supports_local_equalizer()
    }

    fn local_equalizer_entry(&self) -> Option<i64> {
        self.inner.search_engine().local_equalizer_entry()
    }

    fn pump_local_equalizer(&mut self) -> Result<()> {
        self.inner.search_engine_mut().pump_local_equalizer()
    }

    fn apply_local_equalizer(
        &mut self,
        settings: cumulus_core::effects::equalizer::EqualizerSettings,
    ) -> Result<()> {
        self.inner
            .search_engine_mut()
            .apply_local_equalizer(settings)
    }

    fn local_equalizer_processing(&mut self) -> Result<bool> {
        self.inner.search_engine_mut().local_equalizer_processing()
    }

    fn seek_with_receipt(
        &mut self,
        seconds: f64,
        entry: i64,
        cancel: &Cancellation,
    ) -> Result<Option<LoadedMedia>> {
        let graph = self.inner.search_engine().local_equalizer_entry() == Some(entry);
        let result = self
            .inner
            .search_engine_mut()
            .seek_with_receipt(seconds, entry, cancel)?;
        if std::env::var_os("CUMULUS_SEARCH_DOWNLOAD_EQ_LIBRARY").is_some() {
            TRACE.lock().unwrap().seeks.push(json!({
                "seconds":seconds,"before":entry,"graph":graph,
                "replacement":result.as_ref().map(|media|media.entry_id),
                "codec":result.as_ref().and_then(|media|media.codec.as_deref()),
            }));
        }
        Ok(result)
    }

    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        let index = {
            let mut trace = TRACE.lock().unwrap();
            let authorized = matches!(&request.source, MediaInput::Url(url) if url.as_str() == ONLINE)
                && trace.online.len() == 1
                && trace.loads.len() == 1;
            let index = trace.loads.len();
            trace.loads.push(json!({
                "kind":if authorized {"authorized_url"} else {"unexpected"},
                "start":request.start, "end":request.end
            }));
            if !authorized {
                return Err(Error::InvalidInput(
                    "unexpected search proof path or URL load",
                ));
            }
            index
        };
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
        let view = self.inner.poll()?;
        if std::env::var_os("CUMULUS_SEARCH_DOWNLOAD_EQ_LIBRARY").is_some() {
            let mut trace = TRACE.lock().unwrap();
            for event in &view.events {
                if let EngineEvent::Ended {
                    entry_id,
                    reason,
                    error,
                } = event
                {
                    trace.ends.push(
                        json!({"entry":entry_id,"reason":format!("{reason:?}"),"error":error}),
                    );
                }
            }
        }
        Ok(view)
    }
}
