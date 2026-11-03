//! Synthetic selection and published download. Cold launch gets a different
//! transport object that cannot issue either an HTTP or media request.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{DownloadCommand, DownloadQuery, DownloadTab},
    },
    download::{
        service::{DownloadOptions, OutputContainer},
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    media::ncm::{HeaderPolicy, NcmReader},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan, WireRoute,
        },
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "custom-protocol")]
use std::sync::{OnceLock, atomic::AtomicBool};

pub const TITLE: &str = "合成冷离线下载";
#[cfg(feature = "custom-protocol")]
const ACCOUNT: &str = "/eapi/w/nuser/account/get";
const OWNER: u64 = 2;
const SONG: u64 = 17;

#[cfg(feature = "custom-protocol")]
#[derive(Default)]
pub struct Offline {
    account: AtomicUsize,
    other: AtomicUsize,
    media: AtomicUsize,
    pinned: AtomicUsize,
    pinned_ncm: AtomicUsize,
    ordinary_loads: AtomicUsize,
    observed_null: AtomicBool,
    pinned_codec: OnceLock<&'static str>,
}
#[cfg(feature = "custom-protocol")]
impl Offline {
    pub fn facts(&self) -> Value {
        json!({
            "accountAttempts":self.account.load(Ordering::Acquire),
            "otherHttpAttempts":self.other.load(Ordering::Acquire),
            "mediaAttempts":self.media.load(Ordering::Acquire),
            "pinnedLoads":self.pinned.load(Ordering::Acquire),
            "pinnedNcmLoads":self.pinned_ncm.load(Ordering::Acquire),
            "ordinaryLoads":self.ordinary_loads.load(Ordering::Acquire),
            "observedNullAudio":self.observed_null.load(Ordering::Acquire),
            "pinnedCodec":self.pinned_codec.get(),
        })
    }
}
#[cfg(feature = "custom-protocol")]
impl HttpBackend for Offline {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let counter = if plan.url.path() == ACCOUNT {
                &self.account
            } else {
                &self.other
            };
            counter.fetch_add(1, Ordering::AcqRel);
            Err(Error::Transport(
                "cold-offline proof forbids every HTTP request".into(),
            ))
        })
    }
}
#[cfg(feature = "custom-protocol")]
impl MediaBackend for Offline {
    fn get(&self, _: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            self.media.fetch_add(1, Ordering::AcqRel);
            Err(Error::Transport(
                "cold-offline proof forbids every media request".into(),
            ))
        })
    }
}

#[derive(Clone, Copy)]
pub enum SeedFormat {
    Flac,
    NcmFlac,
    NcmAac,
}

impl SeedFormat {
    fn encrypted(self) -> bool {
        !matches!(self, Self::Flac)
    }

    fn media(self) -> (&'static str, &'static [u8], &'static str, Quality) {
        match self {
            Self::Flac | Self::NcmFlac => (
                "https://m1.music.126.net/cumulus-cloud-batch-fixture.flac",
                include_bytes!("../../../core/tests/fixtures/tag_audio/seed.flac"),
                "flac",
                Quality::Lossless,
            ),
            Self::NcmAac => (
                "https://m1.music.126.net/cumulus-cold-offline-fixture.m4a",
                include_bytes!("../../../core/tests/fixtures/tag_audio/seed.m4a"),
                "m4a",
                Quality::Standard,
            ),
        }
    }
}

struct Seed {
    format: SeedFormat,
    requests: AtomicUsize,
}

struct SeedBody {
    data: &'static [u8],
    offset: usize,
}

impl MediaBody for SeedBody {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let size = buffer.len().min(self.data.len() - self.offset);
            buffer[..size].copy_from_slice(&self.data[self.offset..self.offset + size]);
            self.offset += size;
            Ok(size)
        })
    }
}

impl MediaBackend for Seed {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let (url, data, _, _) = self.format.media();
            if request.url.as_str() != url || request.offset >= data.len() as u64 {
                return Err(Error::State("cold-offline seed forbids remote media"));
            }
            self.requests.fetch_add(1, Ordering::AcqRel);
            let offset = request.offset as usize;
            let mut headers = BTreeMap::from([
                ("content-length".into(), (data.len() - offset).to_string()),
                ("etag".into(), "\"synthetic-cold-download\"".into()),
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
                body: Box::new(SeedBody { data, offset }),
            })
        })
    }
}
impl HttpBackend for Seed {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, "/api/w/nuser/account/get")?;
            if plan.url != target {
                return Err(Error::Response("unexpected synthetic seed endpoint"));
            }
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(
                    &json!({"code":200,"profile":{"userId":OWNER,"nickname":"合成冷离线账号"}}),
                )?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}
impl Api for Seed {
    fn request<'a>(&'a self, path: &'a str, params: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (url, bytes, suffix, quality) = self.format.media();
            let ncm = self.format.encrypted();
            match path {
                "/api/v3/song/detail" => Ok(json!({"code":200,"songs":[{
                    "id":SONG,"name":TITLE,"ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},
                    "dt":1000,"fee":u8::from(ncm),
                    "privilege":{"fee":u8::from(ncm),"flag":0,"payed":0}
                }]})),
                "/api/song/enhance/download/url/v1" => Ok(json!({"code":200,"data":{
                    "id":SONG,"url":url,"code":200,"type":suffix,"size":bytes.len(),"level":quality.as_str(),
                    "fee":u8::from(ncm),"payed":0
                }})),
                "/api/music-vip-membership/client/vip/info" if ncm => {
                    if params != json!({"userId":OWNER.to_string()}) {
                        return Err(Error::Response("unexpected seed membership owner"));
                    }
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| Error::State("proof clock precedes epoch"))?
                        .as_millis() as u64;
                    Ok(json!({"code":200,"data":{"now":now,"userId":OWNER,
                        "associator":{"expireTime":now+300_000,"vipLevel":1}}}))
                }
                _ => Err(Error::Response(
                    "unexpected synthetic download seed endpoint",
                )),
            }
        })
    }
}

pub async fn seed(root: &Path, format: SeedFormat) -> Result<Value> {
    let ncm = format.encrypted();
    let (_, bytes, suffix, quality) = format.media();
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
    let seed = Arc::new(Seed {
        format,
        requests: AtomicUsize::new(0),
    });
    let session = Arc::new(AccountSession::new(seed.clone(), Some(registry.store())));
    let candidate = registry.candidate_with_backend(
        ClientProfile::default(),
        Default::default(),
        None,
        seed.clone(),
    )?;
    let url = ApiOrigin::Pc.request_url(WireRoute::Eapi, "/api/w/nuser/account/get")?;
    let credentials = candidate.credentials();
    let revision = credentials.request_credentials(&url)?.revision;
    credentials.record_response(
        &url,
        &["MUSIC_U=synthetic-cold-offline-proof; Path=/; Secure".into()],
        None,
        &revision,
    )?;
    candidate
        .verify_and_adopt(&session, &session.token()?)
        .await?;
    // Use the normal downloader against the bounded in-memory catalog without
    // replacing the saved native selection in the registry.
    session.adopt(seed.clone(), json!({"userId":OWNER}), &session.token()?)?;
    let epoch = session.token()?.generation.to_string();
    let client = MusicClient::with_session(session, registry).with_download_backend(seed.clone());
    let media_dir = root.join("media");
    client
        .download_command(
            &epoch,
            DownloadCommand::Configure {
                root: media_dir
                    .to_str()
                    .ok_or(Error::InvalidInput("proof path is not UTF-8"))?
                    .into(),
                path_type: 2,
                file_type: 1,
            },
        )
        .await?;
    let id = client
        .download_command(
            &epoch,
            DownloadCommand::Start {
                song_ids: vec![SONG.to_string()],
                options: DownloadOptions {
                    tags: false,
                    cover: false,
                    lyrics: false,
                    container: if ncm {
                        OutputContainer::Ncm
                    } else {
                        OutputContainer::Audio
                    },
                    ..Default::default()
                },
            },
        )
        .await?
        .accepted
        .into_iter()
        .next()
        .ok_or(Error::State("synthetic download was not accepted"))?;
    let published = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = client
                .downloads(
                    &epoch,
                    DownloadQuery {
                        tab: DownloadTab::Completed,
                        ..Default::default()
                    },
                )
                .await?;
            if let Some(row) = view
                .tasks
                .into_iter()
                .find(|row| row.id == id && !row.cleanup_pending)
            {
                break Ok::<_, Error>(row);
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .map_err(|_| Error::State("synthetic download did not publish"))??;
    let path = published
        .path
        .ok_or(Error::State("synthetic download has no published path"))?;
    let path = Path::new(&path).canonicalize()?;
    if !path.starts_with(media_dir.canonicalize()?) {
        return Err(Error::State("synthetic publication escaped media root"));
    }
    let decoded = if ncm {
        let mut reader = NcmReader::open(&path, HeaderPolicy::Strict)?;
        let mut decoded = Vec::new();
        reader.read_to_end(&mut decoded)?;
        decoded
    } else {
        std::fs::read(path)?
    };
    if decoded != bytes {
        return Err(Error::State(
            "synthetic published audio does not match fixture",
        ));
    }
    Ok(
        json!({"published":true,"id":id,"title":TITLE,"mediaTransfers":seed.requests.load(Ordering::Acquire),"ncm":ncm,"format":suffix,"quality":quality.as_str()}),
    )
}

#[cfg(feature = "custom-protocol")]
mod cold_audio {
    use super::*;
    use cumulus_core::playback::engine::*;

    pub struct Audio {
        engine: MpvEngine,
        facts: Arc<Offline>,
    }

    impl Audio {
        fn record_codec(&self, loaded: &LoadedMedia) -> Result<()> {
            let codec = match loaded.codec.as_deref() {
                Some("flac") => "flac",
                Some("aac") => "aac",
                _ => return Err(Error::State("unexpected native fixture codec")),
            };
            self.facts
                .pinned_codec
                .set(codec)
                .map_err(|_| Error::State("fixture already decoded"))?;
            Ok(())
        }

        pub fn open(facts: Arc<Offline>) -> Result<Self> {
            Ok(Self {
                engine: MpvEngine::open(MpvConfig {
                    null_output: true,
                    ..Default::default()
                })?,
                facts,
            })
        }
    }

    impl AudioEngine for Audio {
        fn load(&mut self, _: LoadRequest, _: &Cancellation) -> Result<LoadedMedia> {
            self.facts.ordinary_loads.fetch_add(1, Ordering::AcqRel);
            Err(Error::State(
                "cold-offline proof requires a pinned audio handle",
            ))
        }

        fn load_pinned_audio(
            &mut self,
            file: std::fs::File,
            authority: Cancellation,
            cancel: &Cancellation,
        ) -> Result<LoadedMedia> {
            let loaded = self.engine.load_pinned_audio(file, authority, cancel)?;
            self.record_codec(&loaded)?;
            self.facts.pinned.fetch_add(1, Ordering::AcqRel);
            Ok(loaded)
        }

        fn load_pinned_ncm(
            &mut self,
            file: std::fs::File,
            authority: Cancellation,
            cancel: &Cancellation,
        ) -> Result<LoadedMedia> {
            let loaded = self.engine.load_pinned_ncm(file, authority, cancel)?;
            self.record_codec(&loaded)?;
            self.facts.pinned_ncm.fetch_add(1, Ordering::AcqRel);
            Ok(loaded)
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
            let value = self.engine.poll()?;
            if value.audio_output.as_deref() == Some("null") {
                self.facts.observed_null.store(true, Ordering::Release);
            }
            Ok(value)
        }
    }
}
#[cfg(feature = "custom-protocol")]
pub use cold_audio::Audio;
