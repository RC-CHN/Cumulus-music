//! Shared sealed downloader seed, using production verification/publication.
//! This module is example-only and never exposes alternate production grants.
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
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub const TITLE: &str = "合成冷离线下载";
const OWNER: u64 = 2;
const SONG: u64 = 17;

#[derive(Clone, Copy)]
#[allow(
    dead_code,
    reason = "individual example binaries select different sealed fixture lanes"
)]
pub enum SeedFormat {
    Flac,
    NcmFlac,
    NcmAac,
    NcmStereoPcm,
}

impl SeedFormat {
    fn encrypted(self) -> bool {
        !matches!(self, Self::Flac)
    }

    fn media(self) -> (&'static str, &'static [u8], &'static str, Quality) {
        match self {
            Self::Flac | Self::NcmFlac => (
                "https://m1.music.126.net/cumulus-cloud-batch-fixture.flac",
                include_bytes!("../../tests/fixtures/tag_audio/seed.flac"),
                "flac",
                Quality::Lossless,
            ),
            Self::NcmAac => (
                "https://m1.music.126.net/cumulus-cold-offline-fixture.m4a",
                include_bytes!("../../tests/fixtures/tag_audio/seed.m4a"),
                "m4a",
                Quality::Standard,
            ),
            Self::NcmStereoPcm => (
                "https://m1.music.126.net/cumulus-paced-stereo-fixture.flac",
                include_bytes!("../../tests/fixtures/stereo-pcm-source.flac"),
                "flac",
                Quality::Lossless,
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
                    "dt":if matches!(self.format,SeedFormat::NcmStereoPcm){2001}else{1000},"fee":u8::from(ncm),
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

// Desktop fixtures retain the public summary; native held-source proofs also
// need the authoritative completed row's path, without guessing its layout.
#[allow(dead_code)]
pub async fn seed(root: &Path, format: SeedFormat) -> Result<Value> {
    Ok(seed_with_path(root, format).await?.0)
}

pub async fn seed_with_path(root: &Path, format: SeedFormat) -> Result<(Value, PathBuf)> {
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
        std::fs::read(&path)?
    };
    if decoded != bytes {
        return Err(Error::State(
            "synthetic published audio does not match fixture",
        ));
    }
    Ok((
        json!({"published":true,"id":id,"title":TITLE,"mediaTransfers":seed.requests.load(Ordering::Acquire),"ncm":ncm,"format":suffix,"quality":quality.as_str()}),
        path,
    ))
}
