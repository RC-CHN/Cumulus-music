#![allow(dead_code)]
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    cloud::oss::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use sha2::Digest;
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
pub const PART: usize = 100 * 1024;
pub struct NoAccountApi;
impl Api for NoAccountApi {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("OSS transport must not request account APIs") })
    }
}
pub fn allocation(token: &str) -> OssAllocation {
    OssAllocation::parse(&raw(token)).unwrap()
}
pub fn raw(token: &str) -> Value {
    json!({"channel":3,"bucket":"fixture-bucket","objectKey":"audio.flac","endpoint":"oss-cn-hangzhou.aliyuncs.com","resourceId":"resource","accessKeyId":"fixture-access","secretAccessKey":"fixture-secret","token":token})
}
pub enum Fault {
    Reject(u16, &'static str),
    Lost,
    WrongEtag,
    WrongCrc,
    MissingCrc,
    ZeroCrc,
}
pub struct Gate {
    pub entered: tokio::sync::Notify,
    pub release: tokio::sync::Semaphore,
}
impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        })
    }
}
#[derive(Clone)]
pub struct Event {
    pub kind: OssRequestKind,
    pub number: Option<u32>,
    pub token: String,
    pub url: String,
}
#[derive(Default)]
pub struct State {
    pub events: Vec<Event>,
    pub parts: BTreeMap<u32, Vec<u8>>,
    pub object: Option<Vec<u8>>,
    pub marker: String,
    pub active_upload: bool,
    pub required_token: Option<String>,
    pub faults: VecDeque<(OssRequestKind, Option<u32>, Fault)>,
    pub page_size: usize,
}
#[derive(Default)]
pub struct Backend {
    pub state: Mutex<State>,
    pub active: AtomicUsize,
    pub maximum: AtomicUsize,
    pub gate: Mutex<Option<(OssRequestKind, Arc<Gate>)>>,
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
fn http(status: u16, code: &str) -> Error {
    Error::HttpStatus {
        endpoint: None,
        status,
        body: Some(Box::new(json!({"code":code}))),
    }
}
fn xml(root: &str, extra: &str) -> Vec<u8> {
    format!("<{root}><Bucket>fixture-bucket</Bucket><Key>audio.flac</Key>{extra}</{root}>")
        .into_bytes()
}
impl Backend {
    pub fn fault(&self, kind: OssRequestKind, number: Option<u32>, fault: Fault) {
        self.state
            .lock()
            .unwrap()
            .faults
            .push_back((kind, number, fault));
    }
    pub fn events(&self) -> Vec<Event> {
        self.state.lock().unwrap().events.clone()
    }
    fn handle(&self, request: &OssRequest) -> Result<OssReply> {
        let url = url::Url::parse(request.url()).unwrap();
        let params = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
        let kind = request.kind();
        let number = params.get("partNumber").map(|s| s.parse::<u32>().unwrap());
        let token = request.headers()["x-oss-security-token"].clone();
        let mut state = self.state.lock().unwrap();
        state.events.push(Event {
            kind,
            number,
            token: token.clone(),
            url: request.url().into(),
        });
        if state.required_token.as_ref().is_some_and(|s| *s != token) {
            return Err(http(403, "SecurityTokenExpired"));
        }
        let index = state
            .faults
            .iter()
            .position(|(k, n, _)| *k == kind && (n.is_none() || *n == number));
        let fault = index
            .and_then(|i| state.faults.remove(i))
            .map(|(_, _, f)| f);
        if let Some(Fault::Reject(status, code)) = fault {
            return Err(http(status, code));
        }
        let mut response = OssReply {
            status: 200,
            headers: BTreeMap::new(),
            body: Vec::new(),
            digest: None,
        };
        match kind {
            OssRequestKind::Initiate => {
                assert!(!state.active_upload);
                state.active_upload = true;
                state.marker = request.headers()["x-oss-meta-cumulus-upload"].clone();
                response.body = xml(
                    "InitiateMultipartUploadResult",
                    "<UploadId>owned-upload</UploadId>",
                );
            }
            OssRequestKind::Part => {
                if !state.active_upload {
                    return Err(http(404, "NoSuchUpload"));
                }
                assert_eq!(params["uploadId"], "owned-upload");
                let data = request.body().unwrap().to_vec();
                let etag = hex::encode(md5::Md5::digest(&data));
                state.parts.insert(number.unwrap(), data);
                response.headers.insert("etag".into(), etag);
            }
            OssRequestKind::Parts => {
                if !state.active_upload {
                    return Err(http(404, "NoSuchUpload"));
                }
                assert_eq!(params["uploadId"], "owned-upload");
                let marker = params
                    .get("part-number-marker")
                    .map(|s| s.parse::<u32>().unwrap())
                    .unwrap_or(0);
                let all = state
                    .parts
                    .iter()
                    .filter(|(n, _)| **n > marker)
                    .collect::<Vec<_>>();
                let limit = if state.page_size == 0 {
                    1000
                } else {
                    state.page_size
                };
                let page = all.iter().take(limit).collect::<Vec<_>>();
                let more = all.len() > page.len();
                let mut fields =
                    format!("<UploadId>owned-upload</UploadId><IsTruncated>{more}</IsTruncated>");
                if more {
                    fields.push_str(&format!(
                        "<NextPartNumberMarker>{}</NextPartNumberMarker>",
                        page.last().unwrap().0
                    ));
                }
                for (n, data) in page {
                    fields.push_str(&format!("<Part><PartNumber>{n}</PartNumber><Size>{}</Size><ETag>\"{}\"</ETag></Part>",data.len(),hex::encode(md5::Md5::digest(data))));
                }
                response.body = xml("ListPartsResult", &fields);
            }
            OssRequestKind::Put => {
                if state.object.is_some() {
                    return Err(http(409, "FileAlreadyExists"));
                }
                state.marker = request.headers()["x-oss-meta-cumulus-upload"].clone();
                let data = request.body().unwrap().to_vec();
                response
                    .headers
                    .insert("etag".into(), hex::encode(md5::Md5::digest(&data)));
                response
                    .headers
                    .insert("x-oss-hash-crc64ecma".into(), crc64(&data, 0).to_string());
                state.object = Some(data);
            }
            OssRequestKind::Complete => {
                if !state.active_upload {
                    return Err(http(404, "NoSuchUpload"));
                }
                assert_eq!(params["uploadId"], "owned-upload");
                let body = std::str::from_utf8(request.body().unwrap()).unwrap();
                for (n, data) in &state.parts {
                    assert!(body.contains(&format!(
                        "<PartNumber>{n}</PartNumber><ETag>\"{}\"",
                        hex::encode(md5::Md5::digest(data)).to_ascii_uppercase()
                    )));
                }
                let data = state.parts.values().flatten().copied().collect::<Vec<_>>();
                response
                    .headers
                    .insert("x-oss-hash-crc64ecma".into(), crc64(&data, 0).to_string());
                response.body = xml(
                    "CompleteMultipartUploadResult",
                    "<ETag>\"multipart-etag-3\"</ETag>",
                );
                state.object = Some(data);
                state.active_upload = false;
            }
            OssRequestKind::Head | OssRequestKind::Verify => {
                let data = state
                    .object
                    .as_ref()
                    .ok_or_else(|| http(404, "NoSuchKey"))?;
                response
                    .headers
                    .insert("content-length".into(), data.len().to_string());
                response
                    .headers
                    .insert("x-oss-meta-cumulus-upload".into(), state.marker.clone());
                if kind == OssRequestKind::Verify {
                    response.digest = Some(ObjectDigest {
                        bytes: data.len() as u64,
                        md5: hex::encode(md5::Md5::digest(data)),
                        sha256: hex::encode(sha2::Sha256::digest(data)),
                    });
                }
            }
            OssRequestKind::Abort => {
                if !state.active_upload {
                    return Err(http(404, "NoSuchUpload"));
                }
                assert_eq!(params["uploadId"], "owned-upload");
                state.active_upload = false;
                state.parts.clear();
                response.status = 204;
            }
        }
        match fault {
            Some(Fault::Lost) => return Err(Error::Transport("fixture lost OSS reply".into())),
            Some(Fault::WrongEtag) => {
                response.headers.insert("etag".into(), "0".repeat(32));
            }
            Some(Fault::WrongCrc) => {
                response
                    .headers
                    .insert("x-oss-hash-crc64ecma".into(), "1".into());
            }
            Some(Fault::MissingCrc) => {
                response.headers.remove("x-oss-hash-crc64ecma");
            }
            Some(Fault::ZeroCrc) => {
                response
                    .headers
                    .insert("x-oss-hash-crc64ecma".into(), "0".into());
            }
            _ => {}
        }
        Ok(response)
    }
}
impl OssBackend for Backend {
    fn request(&self, request: OssRequest) -> ApiFuture<'_, OssReply> {
        Box::pin(async move {
            let kind = request.kind();
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            let _active = Active(&self.active);
            let response = self.handle(&request);
            let gate = {
                let mut gate = self.gate.lock().unwrap();
                if gate.as_ref().is_some_and(|(k, _)| *k == kind) {
                    gate.take().map(|(_, g)| g)
                } else {
                    None
                }
            };
            if let Some(gate) = gate {
                gate.entered.notify_one();
                gate.release.acquire().await.unwrap().forget();
            }
            if kind == OssRequestKind::Part {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            response
        })
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub source: PathBuf,
    pub data: Vec<u8>,
    pub session: Arc<AccountSession>,
    pub store: Arc<StateStore>,
    pub backend: Arc<Backend>,
    pub manager: OssUploads,
}
impl Fixture {
    pub fn new(size: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("input.flac");
        let data = (0..size).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        std::fs::write(&source, &data).unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(NoAccountApi);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api, json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let backend = Arc::new(Backend::default());
        let manager = OssUploads::with_backend(session.clone(), backend.clone(), PART, 3).unwrap();
        Self {
            root,
            source,
            data,
            session,
            store,
            backend,
            manager,
        }
    }
    pub async fn create(&self) -> String {
        self.manager
            .create(
                Some("owned-task"),
                &self.source,
                allocation("old-token"),
                &hex::encode(md5::Md5::digest(&self.data)),
            )
            .await
            .unwrap()
            .id
    }
    pub fn reopened(&self) -> OssUploads {
        let store = Arc::new(StateStore::open(self.store.directory()).unwrap());
        let api = Arc::new(NoAccountApi);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
        session
            .adopt(api, json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        OssUploads::with_backend(session, self.backend.clone(), 2 * PART, 1).unwrap()
    }
}
