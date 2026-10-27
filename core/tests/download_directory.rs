mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    download::{
        completed::{CompletedDownloads, FileAction, FileEvent, FileOptions},
        directory::*,
        service::{
            DownloadAction, DownloadDestination as Destination, DownloadOptions as Options,
            DownloadRunOptions as Run, DownloadScheduler, DownloadService,
            DownloadStatus as Status, DownloadTask,
        },
        transfer::TransferManager,
    },
    storage::StateStore,
};
use serde_json::json;
use std::{fs, path::PathBuf, sync::Arc};
const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    target: PathBuf,
    service: DownloadService,
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    api: Arc<support::download::Server>,
    backend: Arc<support::download::Backend>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (service, session, store, api, backend) =
            support::download::setup(root.path(), FLAC.into(), "flac");
        let source = root.path().join("media");
        let target = root.path().join("moved");
        let f = Self {
            root,
            source,
            target,
            service,
            session,
            store,
            api,
            backend,
        };
        f.directory().configure(&f.source, 2, 1).unwrap();
        f
    }
    fn directory(&self) -> DownloadDirectory {
        DownloadDirectory::new(self.session.clone()).unwrap()
    }
    fn reopen(&self) -> DownloadDirectory {
        DownloadDirectory::offline(
            Arc::new(StateStore::open(self.store.directory()).unwrap()),
            "1",
        )
        .unwrap()
    }
    fn queued(&self, name: &str) -> DownloadTask {
        self.service
            .create(
                &json!(123),
                Destination::Path {
                    path: self.source.join(name),
                },
                Options {
                    tags: false,
                    cover: false,
                    lyrics: true,
                    ..Default::default()
                },
            )
            .unwrap()
    }
    async fn completed(&self, name: &str) -> DownloadTask {
        self.service
            .run(&self.queued(name).id, Run::default())
            .await
            .unwrap()
    }
    async fn partial(&self, name: &str) -> DownloadTask {
        self.service
            .run(
                &self.queued(name).id,
                Run {
                    byte_budget: Some(11),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
    }
    fn calls(&self) -> usize {
        self.api.calls.lock().unwrap().len()
    }
}
fn event_options(event: DirectoryEvent) -> DirectoryOptions {
    DirectoryOptions {
        observer: Some(Arc::new(move |seen, _| {
            if seen == event {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        })),
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_managed_raw_worker_obeys_the_gate_and_is_waited_for_before_moving() {
    let f = Fixture::new();
    let task = f.partial("raw.flac").await;
    let raw = Arc::new(
        TransferManager::with_backend(
            f.store.clone(),
            "1".into(),
            f.backend.clone(),
            512 * 1024 * 1024,
        )
        .unwrap(),
    );
    let gate = Arc::new(support::transfer::ReadGate::default());
    f.backend.audio.settings.lock().unwrap().gate = Some(gate.clone());
    let worker = {
        let raw = raw.clone();
        let id = task.id.clone();
        tokio::spawn(async move { raw.run(&id, Default::default()).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.reached.notified())
        .await
        .unwrap();
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    let waiting = d.resume(&b.id).unwrap();
    assert_eq!(waiting.phase, MigrationPhase::Quiescing);
    assert_eq!(waiting.waiting, vec![task.id.clone()]);
    gate.release.notify_one();
    let paused = worker.await.unwrap().unwrap();
    assert_eq!(
        paused.status,
        cumulus_core::download::transfer::TransferStatus::Paused
    );
    assert_eq!(paused.bytes, 11);
    assert_eq!(d.resume(&b.id).unwrap().phase, MigrationPhase::Complete);
    f.backend.audio.settings.lock().unwrap().gate = None;
    assert_eq!(
        f.service
            .run(&task.id, Run::default())
            .await
            .unwrap()
            .status,
        Status::Complete
    );
    assert_eq!(f.backend.audio.calls().last().unwrap().offset, 11);
}
fn file_options(event: FileEvent, copy: bool) -> DirectoryOptions {
    DirectoryOptions {
        files: FileOptions {
            copy_only: copy,
            observer: Some(Arc::new(move |seen, _| {
                if seen == event {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            })),
        },
        ..Default::default()
    }
}

#[test]
fn configuration_defaults_validation_and_empty_migration_are_durable() {
    let f = Fixture::new();
    let d = f.directory();
    let settings = d.settings().unwrap().unwrap();
    assert_eq!((settings.path_type, settings.file_type), (2, 1));
    assert!(d.configure(f.store.directory(), 0, 0).is_err());
    assert!(d.configure(&f.source, 3, 0).is_err());
    assert!(d.start(&f.source, None).is_err());
    assert!(d.start(&f.source.join("nested"), None).is_err());
    let b = d.start(&f.target, None).unwrap();
    assert!(
        f.service
            .create_default(&json!(123), Options::default())
            .is_err()
    );
    assert!(d.configure(&f.target, 0, 0).is_err());
    assert_eq!(
        f.reopen().resume(&b.id).unwrap().phase,
        MigrationPhase::Complete
    );
    assert_eq!(d.settings().unwrap().unwrap().root, f.target);
    let task = f
        .service
        .create_default(&json!(123), Options::default())
        .unwrap();
    match task.destination {
        Destination::Named {
            root,
            path_type,
            file_type,
        } => {
            assert_eq!(root, f.target);
            assert_eq!((path_type, file_type), (2, 1));
        }
        _ => panic!(),
    }
    assert_eq!(f.calls(), 0);
    assert!(d.cancel(&b.id).is_err());
}

#[tokio::test]
async fn completed_partial_and_unstarted_named_tasks_move_preserving_relative_paths_and_range() {
    for copy in [false, true] {
        let f = Fixture::new();
        let done = f.completed("album/done.flac").await;
        let partial = f.partial("partial.flac").await;
        let queued = f
            .service
            .create_default(
                &json!(123),
                Options {
                    tags: false,
                    cover: false,
                    lyrics: false,
                    ..Default::default()
                },
            )
            .unwrap();
        let cancelled = f.queued("cancelled.flac");
        f.service
            .request(&cancelled.id, DownloadAction::Cancel)
            .unwrap();
        let external = f
            .service
            .create(
                &json!(123),
                Destination::Path {
                    path: f.root.path().join("outside/external.flac"),
                },
                Options::default(),
            )
            .unwrap();
        fs::write(f.source.join("unmanaged.txt"), b"keep").unwrap();
        let calls = f.calls();
        let d = f.directory().with_options(DirectoryOptions {
            files: FileOptions {
                copy_only: copy,
                ..Default::default()
            },
            ..Default::default()
        });
        let b = d.start(&f.target, None).unwrap();
        assert!(f.service.run(&partial.id, Run::default()).await.is_err());
        assert!(f.service.enqueue(&partial.id).is_err());
        assert!(
            f.service
                .create_default(&json!(123), Options::default())
                .is_err()
        );
        assert!(
            CompletedDownloads::new(f.session.clone())
                .unwrap()
                .start(&done.id, FileAction::Forget, None)
                .is_err()
        );
        let raw = TransferManager::with_backend(
            f.store.clone(),
            "1".into(),
            f.backend.clone(),
            512 * 1024 * 1024,
        )
        .unwrap();
        assert!(raw.restart(&partial.id).is_err());
        let view = d.resume(&b.id).unwrap();
        assert_eq!(view.phase, MigrationPhase::Complete);
        assert_eq!(view.items.len(), 3);
        assert!(view.skipped.contains(&cancelled.id));
        assert!(view.skipped.contains(&external.id));
        assert_eq!(f.calls(), calls);
        assert_eq!(fs::read(f.target.join("album/done.flac")).unwrap(), FLAC);
        assert!(f.target.join("album/done.lrc").exists());
        assert!(!f.source.join("album/done.flac").exists());
        assert_eq!(fs::read(f.source.join("unmanaged.txt")).unwrap(), b"keep");
        assert!(!partial.work.exists());
        assert!(
            f.service
                .get(&partial.id)
                .unwrap()
                .work
                .starts_with(&f.target)
        );
        assert_eq!(f.service.get(&partial.id).unwrap().status, Status::Paused);
        assert_eq!(f.service.get(&queued.id).unwrap().status, Status::Queued);
        assert_eq!(raw.get(&partial.id).unwrap().bytes, 11);
        f.service.run(&partial.id, Run::default()).await.unwrap();
        assert_eq!(f.backend.audio.calls().last().unwrap().offset, 11);
        assert_eq!(fs::read(f.target.join("partial.flac")).unwrap(), FLAC);
        f.service.run(&done.id, Run::default()).await.unwrap();
        assert!(
            !serde_json::to_string(&view)
                .unwrap()
                .contains("auth=fixture")
        );
    }
}

#[tokio::test]
async fn every_child_checkpoint_can_reopen_and_complete_without_restarting_download() {
    for event in [
        FileEvent::Prepared,
        FileEvent::CopyChunk,
        FileEvent::Staged,
        FileEvent::DestinationReady,
        FileEvent::MetadataCommitted,
        FileEvent::SourceRemoved,
        FileEvent::Finished,
    ] {
        for working in [false, true] {
            let f = Fixture::new();
            let task = if working {
                f.partial("audio.flac").await
            } else {
                f.completed("audio.flac").await
            };
            let d = f.directory().with_options(file_options(event, true));
            let b = d.start(&f.target, None).unwrap();
            assert!(d.resume(&b.id).is_err(), "{event:?},working={working}");
            assert!(d.active().unwrap().is_some());
            assert_eq!(d.settings().unwrap().unwrap().root, f.source);
            assert_eq!(
                f.reopen().resume(&b.id).unwrap().phase,
                MigrationPhase::Complete
            );
            if working {
                f.service.run(&task.id, Run::default()).await.unwrap();
                assert_eq!(f.backend.audio.calls().last().unwrap().offset, 11);
            }
            assert_eq!(fs::read(f.target.join("audio.flac")).unwrap(), FLAC);
        }
    }
}

#[tokio::test]
async fn batch_checkpoints_and_final_commit_failure_preserve_gate_and_previous_settings() {
    for event in [
        DirectoryEvent::Planned,
        DirectoryEvent::ItemComplete,
        DirectoryEvent::BeforeFinish,
    ] {
        let f = Fixture::new();
        f.completed("one.flac").await;
        let d = f.directory().with_options(event_options(event));
        let b = d.start(&f.target, None).unwrap();
        assert!(d.resume(&b.id).is_err());
        assert_eq!(d.settings().unwrap().unwrap().root, f.source);
        assert!(f.reopen().resume(&b.id).is_ok());
        assert!(d.active().unwrap().is_none());
    }
    let f = Fixture::new();
    f.completed("one.flac").await;
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_finish BEFORE INSERT ON state WHEN NEW.kind='download_migrations_v1' AND json_extract(NEW.value,'$.view.phase')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(d.resume(&b.id).is_err());
    assert!(f.target.join("one.flac").exists());
    assert!(d.active().unwrap().is_some());
    assert_eq!(d.settings().unwrap().unwrap().root, f.source);
    db.execute_batch("DROP TRIGGER reject_finish;").unwrap();
    assert_eq!(
        f.reopen().resume(&b.id).unwrap().phase,
        MigrationPhase::Complete
    );
}

#[tokio::test]
async fn cancel_reverses_completed_items_and_cancels_uncommitted_work_without_overwrite() {
    for event in [
        DirectoryEvent::Planned,
        DirectoryEvent::ItemComplete,
        DirectoryEvent::BeforeFinish,
    ] {
        let f = Fixture::new();
        f.completed("first.flac").await;
        let partial = f.partial("second.flac").await;
        let d = f.directory().with_options(event_options(event));
        let b = d.start(&f.target, None).unwrap();
        assert!(d.resume(&b.id).is_err());
        let view = f.reopen().cancel(&b.id).unwrap();
        assert_eq!(view.phase, MigrationPhase::Cancelled);
        assert!(f.source.join("first.flac").exists());
        assert_eq!(f.service.get(&partial.id).unwrap().work, partial.work);
        assert_eq!(d.settings().unwrap().unwrap().root, f.source);
        assert!(d.active().unwrap().is_none());
        f.service.run(&partial.id, Run::default()).await.unwrap();
        assert_eq!(f.backend.audio.calls().last().unwrap().offset, 11);
    }
    let f = Fixture::new();
    f.partial("one.flac").await;
    let d = f
        .directory()
        .with_options(file_options(FileEvent::DestinationReady, true));
    let b = d.start(&f.target, None).unwrap();
    assert!(d.resume(&b.id).is_err());
    assert_eq!(
        f.reopen().cancel(&b.id).unwrap().phase,
        MigrationPhase::Cancelled
    );
    assert_eq!(fs::read_dir(&f.target).unwrap().count(), 0);
}

#[tokio::test]
async fn rollback_conflict_retains_foreign_file_and_can_resume_after_resolution() {
    let f = Fixture::new();
    f.completed("one.flac").await;
    let d = f
        .directory()
        .with_options(event_options(DirectoryEvent::ItemComplete));
    let b = d.start(&f.target, None).unwrap();
    assert!(d.resume(&b.id).is_err());
    fs::write(f.source.join("one.flac"), b"foreign").unwrap();
    assert!(f.reopen().cancel(&b.id).is_err());
    assert_eq!(d.get(&b.id).unwrap().phase, MigrationPhase::RollingBack);
    assert_eq!(fs::read(f.source.join("one.flac")).unwrap(), b"foreign");
    assert!(d.active().unwrap().is_some());
    fs::remove_file(f.source.join("one.flac")).unwrap();
    assert_eq!(
        f.reopen().resume(&b.id).unwrap().phase,
        MigrationPhase::Cancelled
    );
    assert_eq!(fs::read(f.source.join("one.flac")).unwrap(), FLAC);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_worker_quiesces_before_manifest_and_user_cancel_survives_migration() {
    let f = Fixture::new();
    let task = f.queued("one.flac");
    let gate = Arc::new(support::transfer::ReadGate::default());
    f.backend.audio.settings.lock().unwrap().gate = Some(gate.clone());
    let service = f.service.clone();
    let id = task.id.clone();
    let worker = tokio::spawn(async move { service.run(&id, Run::default()).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.reached.notified())
        .await
        .unwrap();
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    let waiting = d.resume(&b.id).unwrap();
    assert_eq!(waiting.phase, MigrationPhase::Quiescing);
    assert_eq!(waiting.waiting, vec![task.id.clone()]);
    assert!(waiting.items.is_empty());
    gate.release.notify_one();
    assert_eq!(worker.await.unwrap().unwrap().status, Status::Paused);
    f.service.request(&task.id, DownloadAction::Cancel).unwrap();
    assert!(task.work.exists());
    assert_eq!(d.resume(&b.id).unwrap().phase, MigrationPhase::Complete);
    assert_eq!(f.service.get(&task.id).unwrap().status, Status::Cancelling);
    DownloadScheduler::new(f.service.clone(), 3)
        .unwrap()
        .run_pending()
        .await
        .unwrap();
    assert_eq!(f.service.get(&task.id).unwrap().status, Status::Cancelled);
    assert!(!f.service.get(&task.id).unwrap().work.exists());
}

#[tokio::test]
async fn preflight_rejects_unknown_files_space_shortage_and_collision_without_moving_sources() {
    let empty = Fixture::new();
    let queued = empty.queued("unstarted.flac");
    fs::create_dir(&empty.target).unwrap();
    fs::write(empty.target.join("unstarted.flac"), b"foreign").unwrap();
    let d = empty.directory();
    let b = d.start(&empty.target, None).unwrap();
    assert!(d.resume(&b.id).is_err());
    assert!(queued.work.exists());
    assert_eq!(
        fs::read(empty.target.join("unstarted.flac")).unwrap(),
        b"foreign"
    );
    d.cancel(&b.id).unwrap();
    let f = Fixture::new();
    let task = f.partial("one.flac").await;
    fs::write(task.work.join("foreign"), b"keep").unwrap();
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    assert!(d.resume(&b.id).is_err());
    assert_eq!(d.get(&b.id).unwrap().phase, MigrationPhase::Quiescing);
    fs::remove_file(task.work.join("foreign")).unwrap();
    let tight = f.directory().with_options(DirectoryOptions {
        reserve_bytes: u64::MAX,
        ..Default::default()
    });
    assert!(tight.resume(&b.id).is_err());
    assert!(task.work.exists());
    fs::write(f.target.join("one.flac"), b"foreign").unwrap();
    assert!(d.resume(&b.id).is_err());
    assert_eq!(fs::read(f.target.join("one.flac")).unwrap(), b"foreign");
    fs::remove_file(f.target.join("one.flac")).unwrap();
    assert_eq!(d.resume(&b.id).unwrap().phase, MigrationPhase::Complete);
}

#[tokio::test]
async fn account_switch_stops_children_and_old_owner_can_recover_offline() {
    let f = Fixture::new();
    f.completed("one.flac").await;
    let session = f.session.clone();
    let api = f.api.clone();
    let d = f.directory().with_options(DirectoryOptions {
        observer: Some(Arc::new(move |event, _| {
            if event == DirectoryEvent::Planned {
                session.adopt(api.clone(), json!({"userId":2}), &session.token()?)?;
            }
            Ok(())
        })),
        ..Default::default()
    });
    let b = d.start(&f.target, None).unwrap();
    assert!(matches!(d.resume(&b.id), Err(Error::StaleResponse)));
    assert!(f.source.join("one.flac").exists());
    let other = DownloadDirectory::new(f.session.clone()).unwrap();
    assert!(other.get(&b.id).is_err());
    assert!(other.active().unwrap().is_none());
    assert_eq!(
        f.reopen().resume(&b.id).unwrap().phase,
        MigrationPhase::Complete
    );
}

#[tokio::test]
async fn pending_file_operation_prevents_migration_and_replaced_roots_are_rejected() {
    let f = Fixture::new();
    let task = f.completed("one.flac").await;
    let files = CompletedDownloads::new(f.session.clone()).unwrap();
    let op = files.start(&task.id, FileAction::Forget, None).unwrap();
    assert!(f.directory().start(&f.target, None).is_err());
    files.cancel(&op.id).unwrap();
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    fs::rename(&f.target, f.root.path().join("kept-root")).unwrap();
    fs::create_dir(&f.target).unwrap();
    assert!(d.resume(&b.id).is_err());
    assert!(f.source.join("one.flac").exists());
    fs::remove_dir(&f.target).unwrap();
    fs::rename(f.root.path().join("kept-root"), &f.target).unwrap();
    assert_eq!(d.cancel(&b.id).unwrap().phase, MigrationPhase::Cancelled);
}

#[tokio::test]
async fn published_audio_and_raw_link_groups_survive_copy_and_lost_database_acknowledgments() {
    for kind in ["download_tasks_v1", "media_transfers_v1"] {
        let f = Fixture::new();
        let task = f.queued("published.flac");
        let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
        // Only fixed test strings enter this SQL; no user data or paths.
        db.execute_batch(&format!("CREATE TRIGGER deny_publish BEFORE INSERT ON state WHEN NEW.kind='{kind}' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;")).unwrap();
        assert!(f.service.run(&task.id, Run::default()).await.is_err());
        db.execute_batch("DROP TRIGGER deny_publish;").unwrap();
        let before = f
            .store
            .get("1", "download_tasks_v1", &task.id)
            .unwrap()
            .unwrap();
        let old_pair = if kind == "download_tasks_v1" {
            (
                PathBuf::from(before["prepared"]["path"].as_str().unwrap()),
                f.source.join("published.flac"),
            )
        } else {
            (
                task.work.join("raw"),
                task.work.join(format!(".cumulus-{}.part", task.id)),
            )
        };
        assert!(same_file::is_same_file(&old_pair.0, &old_pair.1).unwrap());
        let calls = f.backend.audio.calls().len();
        let d = f.directory().with_options(DirectoryOptions {
            files: FileOptions {
                copy_only: true,
                ..Default::default()
            },
            ..Default::default()
        });
        let b = d.start(&f.target, None).unwrap();
        assert_eq!(d.resume(&b.id).unwrap().phase, MigrationPhase::Complete);
        let pair = (
            f.target.join(old_pair.0.strip_prefix(&f.source).unwrap()),
            f.target.join(old_pair.1.strip_prefix(&f.source).unwrap()),
        );
        assert!(same_file::is_same_file(&pair.0, &pair.1).unwrap());
        assert!(!old_pair.0.exists());
        assert_eq!(
            f.service
                .run(&task.id, Run::default())
                .await
                .unwrap()
                .status,
            Status::Complete
        );
        assert_eq!(f.backend.audio.calls().len(), calls);
        assert_eq!(fs::read(f.target.join("published.flac")).unwrap(), FLAC);
    }
}

#[tokio::test]
async fn pending_lyric_cleanup_and_offline_records_relocate_then_finish_at_new_path() {
    for published_lyric in [false, true] {
        let f = Fixture::new();
        let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
        if published_lyric {
            db.execute_batch("CREATE TRIGGER deny_lyric BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.lyrics_status')='complete' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        } else {
            *f.api.lyric.lock().unwrap() = Err(Error::Response("test"));
        }
        let task = f.completed("one.flac").await;
        assert!(task.cleanup_pending);
        let calls = f.backend.audio.calls().len();
        if published_lyric {
            db.execute_batch("DROP TRIGGER deny_lyric;").unwrap();
        } else {
            *f.api.lyric.lock().unwrap() =
                Ok(json!({"code":200,"lrc":{"lyric":"[00:00.00]after migration"}}));
        }
        let d = f.directory().with_options(DirectoryOptions {
            files: FileOptions {
                copy_only: true,
                ..Default::default()
            },
            ..Default::default()
        });
        let b = d.start(&f.target, None).unwrap();
        assert_eq!(d.resume(&b.id).unwrap().phase, MigrationPhase::Complete);
        let rows = f.store.values("1", "offline_music_v1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows.values().next().unwrap()["path"],
            json!(f.target.join("one.flac"))
        );
        let done = f.service.run(&task.id, Run::default()).await.unwrap();
        assert!(!done.cleanup_pending);
        assert!(!done.work.exists());
        assert!(f.target.join("one.lrc").exists());
        assert!(!f.source.join("one.lrc").exists());
        assert_eq!(f.backend.audio.calls().len(), calls);
    }
}

#[tokio::test]
async fn user_pause_and_initially_unconfigured_cancellation_are_preserved() {
    let f = Fixture::new();
    let task = f.queued("one.flac");
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    f.service.request(&task.id, DownloadAction::Pause).unwrap();
    assert_eq!(d.resume(&b.id).unwrap().phase, MigrationPhase::Complete);
    assert_eq!(f.service.get(&task.id).unwrap().status, Status::Paused);
    assert!(
        DownloadScheduler::new(f.service.clone(), 3)
            .unwrap()
            .run_pending()
            .await
            .unwrap()
            .is_empty()
    );
    let other = DownloadDirectory::offline(f.store.clone(), "2").unwrap();
    let empty = f.root.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let b = other
        .start(&f.root.path().join("other"), Some(&empty))
        .unwrap();
    assert_eq!(
        other.cancel(&b.id).unwrap().phase,
        MigrationPhase::Cancelled
    );
    assert!(other.settings().unwrap().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn destination_symlinks_and_foreign_work_replacements_are_never_followed_or_deleted() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.completed("album/nested/one.flac").await;
    fs::create_dir(&f.target).unwrap();
    let foreign = f.root.path().join("foreign");
    fs::create_dir(&foreign).unwrap();
    symlink(&foreign, f.target.join("album")).unwrap();
    let d = f.directory();
    let b = d.start(&f.target, None).unwrap();
    assert!(d.resume(&b.id).is_err());
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);
    fs::remove_file(f.target.join("album")).unwrap();
    d.cancel(&b.id).unwrap();
    let task = f.partial("partial.flac").await;
    let d = f
        .directory()
        .with_options(file_options(FileEvent::MetadataCommitted, true));
    let b = d.start(&f.target, None).unwrap();
    assert!(d.resume(&b.id).is_err());
    // The checkpoint may belong to either task. Find a committed working child.
    let recovered = f.reopen();
    let mut view = recovered.get(&b.id).unwrap();
    if f.service.get(&task.id).unwrap().work == task.work {
        assert!(d.resume(&b.id).is_err());
        view = recovered.get(&b.id).unwrap();
    }
    assert!(view.items.iter().any(|i| i.working));
    let kept = f.root.path().join("kept-work");
    fs::rename(&task.work, &kept).unwrap();
    fs::create_dir(&task.work).unwrap();
    fs::write(task.work.join("foreign"), b"keep").unwrap();
    // Post-commit cleanup retains replacement directories and reports them.
    assert_eq!(
        recovered.resume(&b.id).unwrap().phase,
        MigrationPhase::Complete
    );
    assert_eq!(fs::read(task.work.join("foreign")).unwrap(), b"keep");
    let child = view.items.iter().find(|i| i.task_id == task.id).unwrap();
    let op = CompletedDownloads::offline(f.store.clone(), "1")
        .unwrap()
        .get(&child.operation_id)
        .unwrap();
    assert!(op.old_work_retained);
    assert!(kept.exists());
}
