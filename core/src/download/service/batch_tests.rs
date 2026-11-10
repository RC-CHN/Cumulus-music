use super::*;
use crate::{
    account::AccountSession,
    download::{directory::DownloadDirectory, service::DownloadScheduler},
    membership::Quality,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use std::sync::Mutex;

const AUTH: &str = "/api/song/enhance/download/url/v1";

#[derive(Default)]
struct Wire(Mutex<Vec<(String, String)>>);
impl Api for Wire {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/song/like/get" => Ok(json!({"code":200,"ids":[42]})),
                AUTH => {
                    self.0.lock().unwrap().push((
                        args["id"].as_str().unwrap().into(),
                        args["level"].as_str().unwrap().into(),
                    ));
                    Ok(json!({"code":200,"data":{"code":403}}))
                }
                _ => panic!("unexpected synthetic route {path}"),
            }
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    service: DownloadService,
    wire: Arc<Wire>,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let wire = Arc::new(Wire::default());
        let session = Arc::new(AccountSession::new(wire.clone(), Some(store)));
        session
            .adopt(
                wire.clone(),
                json!({"userId":25}),
                &session.token().unwrap(),
            )
            .unwrap();
        DownloadDirectory::new(session.clone())
            .unwrap()
            .configure(&root.path().join("out"), 0, 0)
            .unwrap();
        AccountResources::new(session.clone())
            .unwrap()
            .refresh(ResourceKind::Liked)
            .await
            .unwrap();
        Self {
            root,
            service: DownloadService::new(session).unwrap(),
            wire,
        }
    }

    fn paused(&self, id: &str) -> DownloadTask {
        let task = self
            .service
            .create(
                &json!(id),
                DownloadDestination::Path {
                    path: self.root.path().join("old-target.flac"),
                },
                DownloadOptions {
                    quality: Quality::Lossless,
                    cover: false,
                    ..Default::default()
                },
            )
            .unwrap();
        self.service
            .request(&task.id, DownloadAction::Pause)
            .unwrap()
    }

    fn observe(&self, task: &DownloadTask) -> Option<ExistingSongAdmission> {
        self.service
            .store
            .read_snapshot(|tx| ExistingSongAdmission::observe(tx, "25", &task.song_id, &task.id))
            .unwrap()
    }

    fn admit(&self, previous: ExistingSongAdmission, receipt: &str) -> Result<Vec<String>> {
        let directory = self.root.path();
        self.service.create_guarded_song_batch(
            GuardedSongBatch {
                receipt: receipt.into(),
                admission: SongBatchAdmission::LikedAnchor("42".into()),
                songs: vec!["31".into(), "41".into(), "32".into()].into(),
                options: DownloadOptions::default(),
                existing: Arc::new(BTreeMap::from([("41".into(), previous)])),
            },
            &|| OperationLock::acquire(directory, "test", "scope"),
            &AtomicBool::new(false),
        )
    }
}

#[tokio::test]
async fn mixed_admission_keeps_old_task_and_schedules_its_new_position() {
    let f = Fixture::new().await;
    let old = f.paused("41");
    let prior = f
        .service
        .create_default(&json!(99), DownloadOptions::default())
        .unwrap();
    let ids = f.admit(f.observe(&old).unwrap(), "first-mixed").unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[1], old.id);
    let later = f
        .service
        .create_default(&json!(100), DownloadOptions::default())
        .unwrap();
    let retained = f.service.get(&old.id).unwrap();
    assert_eq!(retained.created_ms, old.created_ms);
    assert_eq!(retained.work, old.work);
    assert_eq!(retained.bytes, old.bytes);
    assert_eq!(
        serde_json::to_value(&retained.destination).unwrap(),
        serde_json::to_value(&old.destination).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&retained.options).unwrap(),
        serde_json::to_value(&old.options).unwrap()
    );
    let reopened = DownloadService::new(f.service.session.clone()).unwrap();
    let results = DownloadScheduler::new(reopened, 1)
        .unwrap()
        .run_pending()
        .await
        .unwrap();
    assert_eq!(
        results.into_iter().map(|row| row.id).collect::<Vec<_>>(),
        [prior.id, ids[0].clone(), old.id, ids[2].clone(), later.id,]
    );
    assert_eq!(
        *f.wire.0.lock().unwrap(),
        [
            ("99".into(), "standard".into()),
            ("31".into(), "standard".into()),
            ("41".into(), "lossless".into()),
            ("32".into(), "standard".into()),
            ("100".into(), "standard".into()),
        ]
    );
}

#[tokio::test]
async fn newer_identical_pause_and_busy_or_moved_work_cannot_admit_mixed_batch() {
    let f = Fixture::new().await;
    let old = f.paused("41");
    let before = f.observe(&old).unwrap();
    let same = f.service.request(&old.id, DownloadAction::Pause).unwrap();
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        serde_json::to_value(&same).unwrap()
    );
    assert!(matches!(
        f.admit(before, "stale"),
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.service.list().unwrap().len(), 1);
    let lock = f.service.lock(&old.id).unwrap();
    assert!(matches!(
        f.admit(f.observe(&old).unwrap(), "busy"),
        Err(Error::MutationBusy)
    ));
    drop(lock);
    let captured = f.observe(&old).unwrap();
    let moved = f.root.path().join("moved-test-stage");
    std::fs::rename(&old.work, moved).unwrap();
    assert!(f.admit(captured, "moved").is_err());
    assert_eq!(f.service.list().unwrap().len(), 1);
    assert_eq!(
        f.service.get(&old.id).unwrap().status,
        DownloadStatus::Paused
    );
    assert_eq!(
        std::fs::read_dir(f.root.path().join("out"))
            .unwrap()
            .count(),
        0
    );
    assert!(f.wire.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn held_middle_rank_never_allows_later_dispatch_before_postcommit_wake() {
    let f = Fixture::new().await;
    let old = f.paused("41");
    let ids = f.admit(f.observe(&old).unwrap(), "held-batch").unwrap();
    let held = f.service.lock(&old.id).unwrap();
    let scheduler = DownloadScheduler::new(f.service.clone(), 3).unwrap();
    let first = scheduler.run_pending().await.unwrap();
    assert_eq!(
        first.iter().map(|row| &row.id).collect::<Vec<_>>(),
        [&ids[0]]
    );
    assert_eq!(
        f.service.get(&ids[1]).unwrap().status,
        DownloadStatus::Queued
    );
    assert_eq!(
        f.service.get(&ids[2]).unwrap().status,
        DownloadStatus::Queued
    );
    drop(held);
    let result = DownloadScheduler::new(f.service.clone(), 1)
        .unwrap()
        .run_pending()
        .await
        .unwrap();
    assert_eq!(
        result.iter().map(|row| &row.id).collect::<Vec<_>>(),
        [&ids[1], &ids[2]]
    );
    assert_eq!(
        *f.wire.0.lock().unwrap(),
        [
            ("31".into(), "standard".into()),
            ("41".into(), "lossless".into()),
            ("32".into(), "standard".into()),
        ]
    );
}

#[tokio::test]
async fn a_pending_cancel_cannot_be_consumed_as_a_requeue() {
    let f = Fixture::new().await;
    let old = f.paused("41");
    let observed = f.observe(&old).unwrap();
    let held = f.service.lock(&old.id).unwrap();
    // This is the normal control path with a native owner still holding
    // the task: it persists Cancel but cannot clean the work yet.
    let stopped = f.service.request(&old.id, DownloadAction::Cancel).unwrap();
    assert_eq!(stopped.status, DownloadStatus::Paused);
    drop(held);
    assert!(f.observe(&old).is_none());
    assert!(matches!(
        f.admit(observed, "cancel-wins"),
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.service.list().unwrap().len(), 1);
    assert!(f.service.has_transfer_control(&old.id).unwrap());
    assert!(f.wire.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn delayed_pause_settlement_cannot_reapply_a_consumed_control() {
    let f = Fixture::new().await;
    let old = f.paused("41");
    let ids = f.admit(f.observe(&old).unwrap(), "consumed").unwrap();
    assert_eq!(ids[1], old.id);
    let token = f.service.session.token().unwrap();
    let settled = f.service.settle_pending_control(&token, &old.id).unwrap();
    assert_eq!(settled.status, DownloadStatus::Queued);
    assert!(!f.service.has_transfer_control(&old.id).unwrap());
    assert!(f.wire.0.lock().unwrap().is_empty());
}
