//! Synthetic selection and published download. Cold launch gets a different
//! transport object that cannot issue either an HTTP or media request.
#[path = "cloud_batch_media.rs"]
mod media;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{DownloadCommand, DownloadQuery, DownloadTab},
    },
    download::service::DownloadOptions,
    protocol::{
        api::{Api, ApiFuture},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan, WireRoute,
        },
    },
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};

#[cfg(feature = "custom-protocol")]
use cumulus_core::download::transfer::{MediaBackend, MediaReply, MediaRequest};
#[cfg(feature = "custom-protocol")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
    ordinary_loads: AtomicUsize,
    observed_null: AtomicBool,
}
#[cfg(feature = "custom-protocol")]
impl Offline {
    pub fn facts(&self) -> Value {
        json!({
            "accountAttempts":self.account.load(Ordering::Acquire),
            "otherHttpAttempts":self.other.load(Ordering::Acquire),
            "mediaAttempts":self.media.load(Ordering::Acquire),
            "pinnedLoads":self.pinned.load(Ordering::Acquire),
            "ordinaryLoads":self.ordinary_loads.load(Ordering::Acquire),
            "observedNullAudio":self.observed_null.load(Ordering::Acquire),
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

struct Seed;
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
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/v3/song/detail" => Ok(json!({"code":200,"songs":[{
                    "id":SONG,"name":TITLE,"ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},
                    "dt":1000,"fee":0
                }]})),
                "/api/song/enhance/download/url/v1" => Ok(json!({"code":200,"data":{
                    "id":SONG,"url":media::URL,"code":200,"type":"flac","size":media::DATA.len(),"level":"lossless"
                }})),
                _ => Err(Error::Response(
                    "unexpected synthetic download seed endpoint",
                )),
            }
        })
    }
}

pub async fn seed(root: &Path) -> Result<Value> {
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
    let seed = Arc::new(Seed);
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
    session.adopt(seed, json!({"userId":OWNER}), &session.token()?)?;
    let epoch = session.token()?.generation.to_string();
    let client = MusicClient::with_session(session, registry)
        .with_download_backend(Arc::new(media::Backend));
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
    if !path.starts_with(media_dir.canonicalize()?) || std::fs::read(path)? != media::DATA {
        return Err(Error::State(
            "synthetic published bytes do not match fixture",
        ));
    }
    Ok(json!({"published":true,"id":id,"title":TITLE,"mediaTransfers":media::requests()}))
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
            self.facts.pinned.fetch_add(1, Ordering::AcqRel);
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
