use super::transfer::Origin;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::{
        service::DownloadService,
        transfer::{MediaBackend, MediaReply, MediaRequest},
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub struct Server {
    pub resource: Mutex<Value>,
    pub song: Mutex<Value>,
    pub lyric: Mutex<Result<Value>>,
    pub calls: Mutex<Vec<(String, Value)>>,
}
impl Server {
    pub fn new(data: &[u8], format: &str) -> Self {
        Self {
            resource: Mutex::new(
                json!({"id":123,"code":200,"url":"https://m.test.music.126.net/media?auth=fixture","size":data.len(),"md5":hex::encode(Md5::digest(data)),"type":format,"level":"lossless","flag":4,"fee":1,"payed":0}),
            ),
            song: Mutex::new(
                json!({"id":123,"name":"Title","ar":[{"id":2,"name":"Artist"}],"al":{"id":3,"name":"Album","picUrl":"http://p.test.music.126.net/cover?param=20y20"},"dt":1000,"no":1,"disc":"1","privilege":{"flag":8196,"fee":1}}),
            ),
            lyric: Mutex::new(Ok(
                json!({"code":200,"lrc":{"lyric":"[00:00.00]fixture lyrics\n"}}),
            )),
            calls: Mutex::new(Vec::new()),
        }
    }
    pub fn count(&self, part: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.contains(part))
            .count()
    }
}
impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            Ok(match path {
                "/api/song/enhance/download/url/v1" => {
                    assert_eq!(payload["immerseType"], "c51");
                    json!({"code":200,"data":self.resource.lock().unwrap().clone()})
                }
                "/api/v3/song/detail" => {
                    json!({"code":200,"songs":[self.song.lock().unwrap().clone()]})
                }
                "/api/song/lyric/v1" => match &*self.lyric.lock().unwrap() {
                    Ok(v) => v.clone(),
                    Err(_) => return Err(Error::Transport("synthetic lyric failure".into())),
                },
                "/api/music-vip-membership/client/vip/info" => {
                    json!({"code":200,"data":{"now":2000000,"userId":1,"associator":{"expireTime":9999999999999u64,"vipLevel":7}}})
                }
                _ => panic!("unexpected endpoint {path}"),
            })
        })
    }
}
pub struct Backend {
    pub audio: Arc<Origin>,
    pub cover: Arc<Origin>,
    pub http_failures: AtomicUsize,
}
impl Backend {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            audio: Arc::new(Origin::new(data)),
            cover: Arc::new(Origin::new(
                include_bytes!("../fixtures/tag_audio/cover.png").to_vec(),
            )),
            http_failures: AtomicUsize::new(0),
        }
    }
}
impl MediaBackend for Backend {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str().contains("/cover?") {
                assert!(
                    request
                        .url
                        .as_str()
                        .ends_with("imageView&enlarge=1&quality=100&thumbnail=640y640")
                );
                self.cover.get(request).await
            } else if self
                .http_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                Err(Error::HttpStatus {
                    endpoint: None,
                    status: 403,
                    body: None,
                })
            } else {
                self.audio.get(request).await
            }
        })
    }
}
pub fn setup(
    root: &Path,
    data: Vec<u8>,
    format: &str,
) -> (
    DownloadService,
    Arc<AccountSession>,
    Arc<StateStore>,
    Arc<Server>,
    Arc<Backend>,
) {
    let api = Arc::new(Server::new(&data, format));
    let backend = Arc::new(Backend::new(data));
    let store = Arc::new(StateStore::open(root.join("state")).unwrap());
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let service =
        DownloadService::with_backend(session.clone(), backend.clone(), 512 * 1024 * 1024).unwrap();
    (service, session, store, api, backend)
}
