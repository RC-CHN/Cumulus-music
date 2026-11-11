use super::*;
use crate::{
    account::AccountSession,
    download::{
        directory::DownloadDirectory,
        service::DownloadScheduler,
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    membership::Quality,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use std::{
    future::{Future, poll_fn},
    pin::pin,
    sync::Mutex,
    task::Poll,
    time::Duration,
};
use tokio::sync::Notify;

const AUTH: &str = "/api/song/enhance/download/url/v1";

#[derive(Default)]
struct Wire(Mutex<Vec<(String, String)>>, Notify, bool);
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
                    self.1.notify_one();
                    if self.2 && args["id"] == "99" {
                        Ok(json!({"code":200,"data":{"code":200,"id":99,
                            "url":"https://m1.music.126.net/scheduler-wake.flac",
                            "size":2048,"type":"flac","level":"standard"}}))
                    } else {
                        Ok(json!({"code":200,"data":{"code":403}}))
                    }
                }
                "/api/v3/song/detail" if self.2 => Ok(json!({
                    "code":200,"songs":[{"id":99,"name":"held","ar":[],"al":{"id":2,"name":"sealed"}}],
                    "privileges":[{"id":99,"st":0,"fee":0,"dl":320000}]
                })),
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
        Self::with_media(None).await
    }

    async fn with_media(media: Option<Arc<BlockedMedia>>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let wire = Arc::new(Wire(Mutex::default(), Notify::new(), media.is_some()));
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
            service: match media {
                Some(media) => DownloadService::with_backend(session, media, 4_096).unwrap(),
                None => DownloadService::new(session).unwrap(),
            },
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
async fn ordinary_and_receipt_entries_share_the_native_song_admission_in_both_orders() {
    use crate::{download::service::CatalogAdmission, library::LocalLibrary};
    use std::{cell::Cell, sync::mpsc, thread};

    let f = Fixture::new().await;
    let token = f.service.session.token().unwrap();
    let library = LocalLibrary::new(f.service.store.clone()).unwrap();
    let ordinary = |song: &str| {
        f.service.admit_catalog_songs(
            &token,
            &library,
            vec![song.into()],
            DownloadOptions::default(),
            &AtomicBool::new(false),
        )
    };
    let scope = || OperationLock::acquire(f.root.path(), "test", "scope");
    let batch = |receipt: &str, ids: &[&str]| GuardedSongBatch {
        receipt: receipt.into(),
        admission: SongBatchAdmission::LikedAnchor("42".into()),
        songs: ids.iter().map(|id| (*id).into()).collect(),
        options: DownloadOptions::default(),
        existing: Arc::new(BTreeMap::new()),
    };
    let (started, entered) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    thread::scope(|threads| {
        let service = &f.service;
        let scope = &scope;
        let plan = batch("receipt-first", &["31"]);
        let peer = threads.spawn(move || {
            let first = Cell::new(true);
            service.create_guarded_song_batch(
                plan,
                &|| {
                    if first.replace(false) {
                        started.send(()).unwrap();
                        released.recv_timeout(Duration::from_secs(5)).unwrap();
                    }
                    scope()
                },
                &AtomicBool::new(false),
            )
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let concurrent = ordinary("31");
        release.send(()).unwrap();
        assert!(matches!(concurrent, Err(Error::MutationBusy)));
        assert_eq!(peer.join().unwrap().unwrap().len(), 1);
    });
    assert!(matches!(
        ordinary("31").unwrap()[0].1,
        Ok(CatalogAdmission::Waiting)
    ));

    let mut generic = ordinary("61").unwrap();
    let Ok(CatalogAdmission::Admitted(id)) = generic.remove(0).1 else {
        panic!("ordinary entry did not admit a fresh task")
    };
    assert!(matches!(
        f.service.create_guarded_song_batch(
            batch("ordinary-first", &["62", "61"]),
            &scope,
            &AtomicBool::new(false),
        ),
        Err(Error::State("song already has a download"))
    ));
    assert!(matches!(
        f.service.create_guarded_song(
            &json!(61),
            DownloadOptions::default(),
            &|| Ok(()),
            &AtomicBool::new(false),
            || panic!("duplicate single receipt committed"),
        ),
        Err(Error::State("song already has a download"))
    ));
    let tasks = f.service.list().unwrap();
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().any(|task| task.id == id));
    assert_eq!(
        std::fs::read_dir(f.root.path().join("out"))
            .unwrap()
            .count(),
        2
    );
    assert!(f.wire.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn history_download_rechecks_completed_anchor_inside_the_admission_transaction() {
    use std::cell::Cell;

    let f = Fixture::new().await;
    let mut anchor = f
        .service
        .create_default(&json!(42), DownloadOptions::default())
        .unwrap();
    anchor.song = Some(json!({"id":42,"privilege":{"id":42}}));
    anchor.status = DownloadStatus::Complete;
    anchor.phase = DownloadPhase::Complete;
    let store = &f.service.store;
    store
        .put(
            "25",
            KIND,
            &anchor.id,
            &serde_json::to_value(&anchor).unwrap(),
        )
        .unwrap();
    let revision = store
        .read_snapshot(|tx| Ok(HistoryService::snapshot_in(tx, "25", HistoryKind::Song)?.revision))
        .unwrap();
    let admission = |receipt: &str| GuardedSongBatch {
        receipt: receipt.into(),
        admission: SongBatchAdmission::HistoryLikedAnchor {
            anchor: "42".into(),
            source: HistoryBatchSource {
                revision: revision.clone(),
                offline: BTreeMap::from([("42".into(), anchor.id.clone())]),
            },
        },
        songs: vec!["41".into()].into(),
        options: DownloadOptions::default(),
        existing: Arc::new(BTreeMap::new()),
    };
    let checks = Cell::new(0);
    let permission = || {
        checks.set(checks.get() + 1);
        if checks.get() == 2 {
            // Model a ledger writer immediately after the host's final
            // preflight, without changing the history or liked membership.
            let mut changed = anchor.clone();
            changed.song = Some(json!({"id":43}));
            store.put("25", KIND, &anchor.id, &serde_json::to_value(changed)?)?;
        }
        OperationLock::acquire(f.root.path(), "test", "scope")
    };
    assert!(matches!(
        f.service.create_guarded_song_batch(
            admission("history-old"),
            &permission,
            &AtomicBool::new(false),
        ),
        Err(Error::QueueChanged)
    ));
    assert_eq!(checks.get(), 2);
    assert_eq!(f.service.list().unwrap().len(), 1);
    assert_eq!(store.get("25", BATCH, LAST).unwrap(), None);
    store
        .put(
            "25",
            KIND,
            &anchor.id,
            &serde_json::to_value(&anchor).unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.service
            .create_guarded_song_batch(
                admission("history-new"),
                &permission,
                &AtomicBool::new(false),
            )
            .unwrap()
            .len(),
        1
    );
    assert!(f.wire.0.lock().unwrap().is_empty());
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

#[derive(Default)]
struct BlockedMedia {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl MediaBackend for BlockedMedia {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(
                request.url.as_str(),
                "https://m1.music.126.net/scheduler-wake.flac"
            );
            assert_eq!(request.offset, 0);
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), "2048".into())]),
                body: Box::new(BlockedBody(self.entered.clone(), self.release.clone())),
            })
        })
    }
}

struct BlockedBody(Arc<Notify>, Arc<Notify>);
impl MediaBody for BlockedBody {
    fn read<'a>(&'a mut self, _output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            self.0.notify_one();
            self.1.notified().await;
            Err(Error::Transport("synthetic blocked body released".into()))
        })
    }
}

#[tokio::test]
async fn live_drain_observes_commit_wake_behind_a_held_head_without_waiting_for_active_body() {
    let media = Arc::new(BlockedMedia::default());
    let f = Fixture::with_media(Some(media.clone())).await;
    let old = f.paused("41");
    let blocker = f
        .service
        .create_default(&json!(99), DownloadOptions::default())
        .unwrap();
    let scheduler = DownloadScheduler::new(f.service.clone(), 2).unwrap();
    let wake = Notify::new();
    let mut draining = pin!(scheduler.run_pending_notified(&wake));
    poll_fn(|cx| {
        assert!(draining.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    tokio::time::timeout(Duration::from_secs(3), media.entered.notified())
        .await
        .unwrap();
    assert_eq!(
        f.service.get(&blocker.id).unwrap().status,
        DownloadStatus::Downloading
    );

    let ids = f
        .admit(f.observe(&old).unwrap(), "wake-behind-held-head")
        .unwrap();
    // Hold the already-committed FIFO head, the exact state visible between
    // the SQL commit and release of the staging locks. Poll directly so the
    // drain observes that state before we send its postcommit notification.
    let head = f.service.lock(&ids[0]).unwrap();
    wake.notify_one();
    poll_fn(|cx| {
        assert!(draining.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(
        f.service.lock(&ids[1]).is_ok(),
        "the held head was bypassed"
    );
    assert_eq!(f.wire.0.lock().unwrap().len(), 1);
    drop(head);
    wake.notify_one();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if f.wire.0.lock().unwrap().len() == 4 {
                break;
            }
            tokio::select! {
                _ = f.wire.1.notified() => (),
                _ = &mut draining => panic!("drain ended with the blocker still held"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        f.service.get(&blocker.id).unwrap().status,
        DownloadStatus::Downloading
    );
    media.release.notify_one();
    let result = draining.await.unwrap();
    assert_eq!(result.len(), 4);
    assert_eq!(
        *f.wire.0.lock().unwrap(),
        [
            ("99".into(), "standard".into()),
            ("31".into(), "standard".into()),
            ("41".into(), "lossless".into()),
            ("32".into(), "standard".into()),
        ]
    );
}

#[tokio::test]
async fn explicit_resume_has_fresh_position_without_reordering_already_queued_work() {
    let f = Fixture::new().await;
    let manual = f.paused("75");
    let old = f.paused("41");
    let ids = f.admit(f.observe(&old).unwrap(), "before-manual").unwrap();
    let resumed = f.service.enqueue(&manual.id).unwrap();
    assert!(resumed.dispatch > manual.dispatch);
    assert_eq!(resumed.created_ms, manual.created_ms);
    assert_eq!(resumed.work, manual.work);
    assert!(f.service.enqueue(&manual.id).unwrap().dispatch == resumed.dispatch);
    let results = DownloadScheduler::new(f.service.clone(), 1)
        .unwrap()
        .run_pending()
        .await
        .unwrap();
    assert_eq!(
        results.into_iter().map(|row| row.id).collect::<Vec<_>>(),
        [ids[0].clone(), old.id, ids[2].clone(), manual.id]
    );
    assert_eq!(
        *f.wire.0.lock().unwrap(),
        [
            ("31".into(), "standard".into()),
            ("41".into(), "lossless".into()),
            ("32".into(), "standard".into()),
            ("75".into(), "lossless".into()),
        ]
    );
}

#[tokio::test]
async fn explicit_resume_does_not_erase_a_durable_cancel() {
    let f = Fixture::new().await;
    let task = f.paused("75");
    let lock = f.service.lock(&task.id).unwrap();
    f.service.request(&task.id, DownloadAction::Cancel).unwrap();
    drop(lock);
    assert!(matches!(
        f.service.enqueue(&task.id),
        Err(Error::State("download cancellation already requested"))
    ));
    let pending = f
        .service
        .store
        .read_snapshot(|tx| Control::observe(tx, "25", &task.id))
        .unwrap();
    assert_eq!(pending.action, Some(DownloadAction::Cancel));
    let after = f.service.get(&task.id).unwrap();
    assert_eq!(after.status, DownloadStatus::Paused);
    assert!(after.dispatch == task.dispatch);
    assert!(f.wire.0.lock().unwrap().is_empty());
}
