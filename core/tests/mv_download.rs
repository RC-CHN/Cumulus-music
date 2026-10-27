mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    download::{
        completed::{CompletedDownloads, FileAction},
        offline::OfflineLibrary,
        service::{
            DownloadDestination as Destination, DownloadOrigin, DownloadRunOptions as Run,
            DownloadService, DownloadStatus as Status,
        },
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{Arc, Mutex, atomic::Ordering},
};
const VIDEO: &[u8] = include_bytes!("fixtures/mv-seed.mp4");
const AUTH: &str = "/api/song/enhance/download/mv/url";
struct Server {
    detail: Mutex<Value>,
    resource: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
}
impl Server {
    fn count(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
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
            match path {
                "/api/mlog/detail/v1" => {
                    assert_eq!(payload, json!({"id":"123","type":"2","videoType":"mp4"}));
                    Ok(json!({"code":200,"data":self.detail.lock().unwrap().clone()}))
                }
                AUTH => {
                    assert_eq!(payload, json!({"id":"123","r":240}));
                    Ok(json!({"code":200,"data":self.resource.lock().unwrap().clone()}))
                }
                _ => panic!("MV must not use song, point or collection endpoint {path}"),
            }
        })
    }
}
struct Fixture {
    root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<Server>,
    session: Arc<AccountSession>,
    media: Arc<support::download::Backend>,
    service: DownloadService,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(Server {
            detail: Mutex::new(
                json!({"id":"123","resource":{"data":{"id":123,"name":"Video","artists":[{"id":7,"name":"Artist"}],"duration":500,"brs":[{"br":240,"size":VIDEO.len()+1,"point":0}]},"mp":{"fee":0,"payed":0,"pl":1080,"dl":1080}}}),
            ),
            resource: Mutex::new(
                json!({"id":123,"code":200,"url":"https://v.test.vod.126.net/video?key=fixture","r":240,"size":VIDEO.len(),"md5":"","fee":0}),
            ),
            calls: Mutex::new(vec![]),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let media = Arc::new(support::download::Backend::new(VIDEO.into()));
        let service =
            DownloadService::with_backend(session.clone(), media.clone(), 1024 * 1024).unwrap();
        Self {
            root,
            store,
            api,
            session,
            media,
            service,
        }
    }
    fn destination(&self) -> Destination {
        Destination::Named {
            root: self.root.path().join("out"),
            path_type: 0,
            file_type: 0,
        }
    }
    async fn create(&self) -> cumulus_core::download::service::DownloadTask {
        self.service
            .create_mv(&json!(123), 240, self.destination())
            .await
            .unwrap()
    }
    fn reopen(&self) -> DownloadService {
        let session = Arc::new(AccountSession::new(
            self.api.clone(),
            Some(Arc::new(
                StateStore::open(self.root.path().join("state")).unwrap(),
            )),
        ));
        session
            .adopt(
                self.api.clone(),
                json!({"userId":1}),
                &session.token().unwrap(),
            )
            .unwrap();
        DownloadService::with_backend(session, self.media.clone(), 1024 * 1024).unwrap()
    }
}
#[tokio::test]
async fn mv_download_keeps_video_bytes_names_resolution_and_all_file_management() {
    let f = Fixture::new();
    let task = f.create().await;
    assert_eq!(task.origin, DownloadOrigin::Mv { resolution: 240 });
    let task = f.service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(task.status, Status::Complete);
    assert!(!task.encrypted && !task.options.tags && !task.options.cover && !task.options.lyrics);
    let path = task.target.as_ref().unwrap();
    assert!(path.ends_with("MV/Artist - Video.mp4"));
    assert_eq!(fs::read(path).unwrap(), VIDEO);
    assert_eq!(task.resource.as_ref().unwrap()["r"], 240);
    assert_eq!(task.resource.as_ref().unwrap()["size"], VIDEO.len());
    assert_eq!(
        task.raw_integrity.as_ref().unwrap().sha256,
        task.output_integrity.as_ref().unwrap().sha256
    );
    let offline = OfflineLibrary::new(f.session.clone()).unwrap();
    let checked = offline
        .check(&f.session.token().unwrap(), path, true)
        .await
        .unwrap();
    assert!(checked.access.can_play && checked.integrity_verified);
    let files = CompletedDownloads::new(f.session.clone()).unwrap();
    let op = files.rename(&task.id, "renamed.mp4").unwrap();
    files.resume(&op.id).unwrap();
    fs::create_dir(f.root.path().join("moved")).unwrap();
    let target = f.root.path().join("moved/final.mp4");
    let op = files
        .start(&task.id, FileAction::Move, Some(target.clone()))
        .unwrap();
    files.resume(&op.id).unwrap();
    assert_eq!(fs::read(&target).unwrap(), VIDEO);
    let op = files.start(&task.id, FileAction::Delete, None).unwrap();
    files.resume(&op.id).unwrap();
    assert!(!target.exists());
    assert!(
        offline
            .records(&f.session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.api.count(AUTH), 1);
}
#[tokio::test]
async fn no_md5_resume_reuses_authorization_across_restart_and_keeps_partial_bytes() {
    let f = Fixture::new();
    let task = f.create().await;
    let paused = f
        .service
        .run(
            &task.id,
            Run {
                byte_budget: Some(257),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.status, Status::Paused);
    assert_eq!(paused.bytes, 257);
    let service = f.reopen();
    service.enqueue(&task.id).unwrap();
    let done = service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(done.authorization_count, 1);
    assert_eq!(f.api.count(AUTH), 1);
    assert_eq!(f.media.audio.calls()[1].offset, 257);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), VIDEO);
}
#[tokio::test]
async fn expired_mv_authorization_refreshes_once_and_discards_unproven_old_bytes() {
    let f = Fixture::new();
    let task = f.create().await;
    f.service
        .run(
            &task.id,
            Run {
                byte_budget: Some(257),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    f.media.http_failures.store(1, Ordering::SeqCst);
    f.api.resource.lock().unwrap()["url"] = json!("https://v.test.vod.126.net/video?key=new");
    f.service.enqueue(&task.id).unwrap();
    let done = f.service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.authorization_count, 2);
    assert_eq!(f.media.audio.calls()[1].offset, 0);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), VIDEO);
    let f = Fixture::new();
    let task = f.create().await;
    f.media.http_failures.store(2, Ordering::SeqCst);
    assert!(matches!(
        f.service.run(&task.id, Run::default()).await,
        Err(Error::HttpStatus { status: 403, .. })
    ));
    assert_eq!(f.api.count(AUTH), 2);
}
#[tokio::test]
async fn paid_points_rights_loss_and_invalid_options_never_fall_back_to_song_or_spend() {
    for (field, value) in [
        ("fee", json!(1)),
        ("dl", json!(0)),
        ("unauthorized", json!(true)),
    ] {
        let f = Fixture::new();
        f.api.detail.lock().unwrap()["resource"]["mp"][field] = value;
        assert!(
            f.service
                .create_mv(&json!(123), 240, f.destination())
                .await
                .is_err()
        );
        assert!(f.service.list().unwrap().is_empty());
        assert_eq!(f.api.count(AUTH), 0);
    }
    let f = Fixture::new();
    f.api.detail.lock().unwrap()["resource"]["data"]["brs"][0]["point"] = json!(2);
    assert!(
        f.service
            .create_mv(&json!(123), 240, f.destination())
            .await
            .is_err()
    );
    assert_eq!(f.api.count(AUTH), 0);
    let f = Fixture::new();
    let task = f.create().await;
    f.api.detail.lock().unwrap()["resource"]["mp"]["dl"] = json!(0);
    assert!(f.service.run(&task.id, Run::default()).await.is_err());
    assert_eq!(f.api.count(AUTH), 0);
    assert!(f.media.audio.calls().is_empty());
    let f = Fixture::new();
    let task = f.create().await;
    let mut stored = f
        .store
        .get("1", "download_tasks_v1", &task.id)
        .unwrap()
        .unwrap();
    stored["options"]["tags"] = json!(true);
    f.store
        .put("1", "download_tasks_v1", &task.id, &stored)
        .unwrap();
    assert!(f.service.run(&task.id, Run::default()).await.is_err());
    assert_eq!(f.api.count(AUTH), 0);
    assert!(f.media.audio.calls().is_empty());
    let f = Fixture::new();
    let task = f.create().await;
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.service.run(&task.id, Run::default()).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.api.count(AUTH), 0);
}
#[tokio::test]
async fn entitled_mv_download_accepts_paid_rights_and_reports_server_resolution() {
    let f = Fixture::new();
    {
        let mut detail = f.api.detail.lock().unwrap();
        detail["resource"]["mp"]["fee"] = json!(1);
        detail["resource"]["mp"]["payed"] = json!(1);
    }
    f.api.resource.lock().unwrap()["r"] = json!(144);
    let task = f.create().await;
    let task = f.service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(task.resource.unwrap()["r"], 144);
    assert_eq!(task.origin, DownloadOrigin::Mv { resolution: 240 });
}
