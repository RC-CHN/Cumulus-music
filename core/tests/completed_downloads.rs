mod support;
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::{
        completed::*,
        offline::OfflineLibrary,
        service::{
            DownloadDestination as Destination, DownloadOptions as Options,
            DownloadRunOptions as Run, DownloadService, DownloadTask, LyricsStatus,
        },
    },
    library::{AudioMetadata, LocalLibrary, identity::file_id, probe::AudioProbe},
    playback::{PlaybackMode, PlaybackQueue},
    storage::{OperationLock, StateStore},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("flac".into()),
            duration: 1.0,
            title: "Test".into(),
            album: "".into(),
            artists: vec![],
            bitrate: None,
            sample_rate: Some(8000),
            channels: Some(1),
            container: "flac".into(),
            cover_bytes: None,
        })
    }
}
struct Fixture {
    root: tempfile::TempDir,
    service: DownloadService,
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    api: Arc<support::download::Server>,
    task: DownloadTask,
    source: PathBuf,
}
impl Fixture {
    async fn new(lyrics: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (service, session, store, api, _) =
            support::download::setup(root.path(), FLAC.into(), "flac");
        let source = root
            .path()
            .canonicalize()
            .unwrap()
            .join("media")
            .join("input.flac");
        let t = service
            .create(
                &json!(123),
                Destination::Path {
                    path: source.clone(),
                },
                Options {
                    tags: false,
                    cover: false,
                    lyrics,
                    ..Default::default()
                },
            )
            .unwrap();
        let task = service.run(&t.id, Run::default()).await.unwrap();
        assert!(!task.cleanup_pending);
        Self {
            root,
            service,
            session,
            store,
            api,
            task,
            source,
        }
    }
    fn files(&self) -> CompletedDownloads {
        CompletedDownloads::new(self.session.clone()).unwrap()
    }
    fn offline(&self) -> CompletedDownloads {
        CompletedDownloads::offline(
            Arc::new(StateStore::open(self.store.directory()).unwrap()),
            "1",
        )
        .unwrap()
    }
    fn target(&self) -> PathBuf {
        self.root
            .path()
            .canonicalize()
            .unwrap()
            .join("moved")
            .join("renamed.flac")
    }
    fn library(&self) -> LocalLibrary {
        LocalLibrary::with_probe(self.store.clone(), Arc::new(Probe)).unwrap()
    }
    fn start(&self, files: &CompletedDownloads) -> FileOperationView {
        files
            .start(&self.task.id, FileAction::Move, Some(self.target()))
            .unwrap()
    }
}
fn interrupt(event: FileEvent) -> FileOptions {
    FileOptions {
        copy_only: false,
        observer: Some(Arc::new(move |seen, _| {
            if seen == event {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        })),
    }
}

#[tokio::test]
async fn legacy_queue_path_aliases_follow_moves_and_deletes() {
    for action in [FileAction::Move, FileAction::Delete] {
        let f = Fixture::new(false).await;
        let alias = f
            .root
            .path()
            .join("media")
            .join("..")
            .join("media")
            .join("input.flac");
        assert_ne!(alias, f.source);
        assert_eq!(alias.canonicalize().unwrap(), f.source);
        let mut queue = PlaybackQueue::new(Some(f.store.clone()), "1").unwrap();
        queue
            .replace(
                vec![json!({"path":alias}), json!({"id":777})],
                0,
                json!(null),
            )
            .unwrap();
        let files = f.files();
        let operation = files
            .start(
                &f.task.id,
                action,
                if action == FileAction::Move {
                    Some(f.target())
                } else {
                    None
                },
            )
            .unwrap();
        assert_eq!(
            files.resume(&operation.id).unwrap().phase,
            FilePhase::Complete
        );
        queue.reload().unwrap();
        let snapshot = queue.snapshot();
        if action == FileAction::Move {
            assert_eq!(snapshot.items[0].resource["path"], json!(f.target()));
        } else {
            assert_eq!(snapshot.items.len(), 1);
            assert_eq!(snapshot.items[0].resource["id"], 777);
        }
    }
}

#[tokio::test]
async fn move_and_copy_update_task_lyrics_offline_library_mappings_and_queues_without_network() {
    for copy in [false, true] {
        let f = Fixture::new(true).await;
        let library = f.library();
        library.scan(f.source.parent().unwrap()).unwrap();
        let old_id = file_id(&f.source).unwrap();
        library
            .map_online(&f.session, &old_id, &json!(123))
            .unwrap();
        let other = AccountSession::new(f.api.clone(), Some(f.store.clone()));
        other
            .adopt(f.api.clone(), json!({"userId":2}), &other.token().unwrap())
            .unwrap();
        library.map_online(&other, &old_id, &json!(124)).unwrap();
        let resource = serde_json::to_value(library.resource(&old_id).unwrap()).unwrap();
        let mut queue = PlaybackQueue::new(Some(f.store.clone()), "1").unwrap();
        queue
            .replace(
                vec![resource, json!({"path":f.source}), json!({"id":999})],
                0,
                json!(null),
            )
            .unwrap();
        queue.set_mode(PlaybackMode::Random).unwrap();
        let before = queue.snapshot();
        f.store
            .put(
                "1",
                "playback",
                "fm-backup-queue",
                &serde_json::to_value(&before).unwrap(),
            )
            .unwrap();
        f.store
            .put(
                "1",
                "playback",
                "fm-backup-position",
                &json!({"entry_id":before.current,"position":12.0}),
            )
            .unwrap();
        let calls = f.api.calls.lock().unwrap().len();
        let files = f.files().with_options(FileOptions {
            copy_only: copy,
            ..Default::default()
        });
        let op = f.start(&files);
        assert!(f.service.run(&f.task.id, Run::default()).await.is_err());
        let view = files.resume(&op.id).unwrap();
        assert_eq!(view.phase, FilePhase::Complete);
        assert!(!f.source.exists());
        assert!(!f.source.with_extension("lrc").exists());
        assert_eq!(fs::read(f.target()).unwrap(), FLAC);
        assert!(f.target().with_extension("lrc").exists());
        assert!(
            !serde_json::to_string(&view)
                .unwrap()
                .contains("auth=fixture")
        );
        let new_id = file_id(&f.target()).unwrap();
        assert!(library.item(&old_id).is_err());
        assert_eq!(
            library.online_id(&f.session, &new_id).unwrap().as_deref(),
            Some("123")
        );
        assert_eq!(
            library.online_id(&other, &new_id).unwrap().as_deref(),
            Some("124")
        );
        queue.reload().unwrap();
        let after = queue.snapshot();
        let backup: cumulus_core::playback::QueueSnapshot = serde_json::from_value(
            f.store
                .get("1", "playback", "fm-backup-queue")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(backup, after);
        assert_eq!(
            f.store
                .get("1", "playback", "fm-backup-position")
                .unwrap()
                .unwrap()["position"],
            12.0
        );
        assert_eq!(after.order, before.order);
        assert_eq!(after.mode, before.mode);
        assert_eq!(after.current, before.current);
        assert_eq!(after.items[0].resource["local_id"], new_id);
        assert_eq!(after.items[1].resource["path"], json!(f.target()));
        let off = OfflineLibrary::new(f.session.clone()).unwrap();
        let records = off.records(&f.session.token().unwrap()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, f.target());
        assert!(
            off.check(&f.session.token().unwrap(), f.target(), false)
                .await
                .unwrap()
                .integrity_verified
        );
        assert_eq!(
            f.service
                .run(&f.task.id, Run::default())
                .await
                .unwrap()
                .target,
            Some(f.target())
        );
        assert_eq!(calls, f.api.calls.lock().unwrap().len());
    }
}

#[tokio::test]
async fn physical_delete_removes_only_owned_audio_and_lyric_and_advances_current() {
    let f = Fixture::new(true).await;
    let library = f.library();
    library.scan(f.source.parent().unwrap()).unwrap();
    let id = file_id(&f.source).unwrap();
    library.map_online(&f.session, &id, &json!(123)).unwrap();
    let mut q = PlaybackQueue::new(Some(f.store.clone()), "1").unwrap();
    q.replace(
        vec![json!({"path":f.source}), json!({"id":777})],
        0,
        json!(null),
    )
    .unwrap();
    f.store
        .put("1", "playback", "position", &json!({"seconds":12}))
        .unwrap();
    let files = f.files();
    let op = files.start(&f.task.id, FileAction::Delete, None).unwrap();
    files.resume(&op.id).unwrap();
    assert!(!f.source.exists());
    assert!(!f.source.with_extension("lrc").exists());
    assert!(f.service.get(&f.task.id).is_err());
    assert!(library.item(&id).is_err());
    q.reload().unwrap();
    assert_eq!(q.selected().unwrap().resource["id"], 777);
    assert!(f.store.get("1", "playback", "position").unwrap().is_none());
    assert!(f.store.values("1", "offline_music_v1").unwrap().is_empty());
    assert!(f.store.values("1", "local_map_v1").unwrap().is_empty());
    assert!(
        f.store
            .get("1", "media_transfers_v1", &f.task.id)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn forget_preserves_files_library_and_queue_but_removes_download_and_offline_records() {
    let f = Fixture::new(true).await;
    let library = f.library();
    library.scan(f.source.parent().unwrap()).unwrap();
    let id = file_id(&f.source).unwrap();
    let mut q = PlaybackQueue::new(Some(f.store.clone()), "1").unwrap();
    q.replace(vec![json!({"path":f.source})], 0, json!(null))
        .unwrap();
    let before = q.snapshot();
    let files = f.offline();
    let op = files.start(&f.task.id, FileAction::Forget, None).unwrap();
    files.resume(&op.id).unwrap();
    assert!(f.source.exists());
    assert!(f.source.with_extension("lrc").exists());
    assert!(library.item(&id).is_ok());
    q.reload().unwrap();
    assert_eq!(q.snapshot(), before);
    assert!(f.store.values("1", "download_tasks_v1").unwrap().is_empty());
    assert!(f.store.values("1", "offline_music_v1").unwrap().is_empty());
}

#[tokio::test]
async fn missing_owned_files_clear_stale_records_and_foreign_sidecars_are_preserved() {
    let f = Fixture::new(false).await;
    fs::write(f.source.with_extension("lrc"), "unmanaged lyric").unwrap();
    fs::remove_file(&f.source).unwrap();
    let files = f.files();
    assert!(
        files
            .start(&f.task.id, FileAction::Move, Some(f.target()))
            .is_err()
    );
    let op = files.start(&f.task.id, FileAction::Delete, None).unwrap();
    assert_eq!(op.missing, vec!["audio"]);
    files.resume(&op.id).unwrap();
    assert_eq!(
        fs::read_to_string(f.source.with_extension("lrc")).unwrap(),
        "unmanaged lyric"
    );
    let f = Fixture::new(true).await;
    fs::remove_file(f.source.with_extension("lrc")).unwrap();
    let op = f.start(&f.files());
    f.files().resume(&op.id).unwrap();
    assert_eq!(
        f.service.get(&f.task.id).unwrap().lyrics_status,
        LyricsStatus::Missing
    );
    assert!(f.service.run(&f.task.id, Run::default()).await.is_ok());
}

#[tokio::test]
async fn all_durable_checkpoints_reopen_and_complete_without_reusing_foreign_files() {
    for copy in [false, true] {
        for event in [
            FileEvent::DestinationReady,
            FileEvent::MetadataCommitted,
            FileEvent::SourceRemoved,
            FileEvent::CopyChunk,
            FileEvent::Staged,
        ] {
            if !copy && matches!(event, FileEvent::CopyChunk | FileEvent::Staged) {
                continue;
            }
            let f = Fixture::new(true).await;
            let mut options = interrupt(event);
            options.copy_only = copy;
            let files = f.files().with_options(options);
            let op = f.start(&files);
            assert!(files.resume(&op.id).is_err(), "{event:?}");
            let phase = files.get(&op.id).unwrap().phase;
            if matches!(
                event,
                FileEvent::MetadataCommitted | FileEvent::SourceRemoved
            ) {
                assert_eq!(phase, FilePhase::Cleanup);
                assert!(files.cancel(&op.id).is_err());
            }
            let reopened = f.offline();
            assert_eq!(reopened.resume(&op.id).unwrap().phase, FilePhase::Complete);
            assert!(!f.source.exists());
            assert_eq!(fs::read(f.target()).unwrap(), FLAC);
            assert!(f.target().with_extension("lrc").exists());
            assert!(
                fs::read_dir(f.target().parent().unwrap())
                    .unwrap()
                    .all(|e| !e
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".cumulus-move-"))
            );
        }
    }
}

#[tokio::test]
async fn cancellation_before_commit_removes_only_staged_outputs_and_preserves_source() {
    for event in [FileEvent::CopyChunk, FileEvent::DestinationReady] {
        let f = Fixture::new(true).await;
        let mut options = interrupt(event);
        options.copy_only = true;
        let files = f.files().with_options(options);
        let op = f.start(&files);
        assert!(files.resume(&op.id).is_err());
        assert_eq!(
            f.offline().cancel(&op.id).unwrap().phase,
            FilePhase::Cancelled
        );
        assert_eq!(fs::read(&f.source).unwrap(), FLAC);
        assert!(f.source.with_extension("lrc").exists());
        assert!(!f.target().exists());
        assert!(f.service.get(&f.task.id).unwrap().file_operation.is_none());
        assert!(f.service.run(&f.task.id, Run::default()).await.is_ok());
    }
}

#[tokio::test]
async fn account_changes_and_shared_locks_stop_operations_but_keep_original_responsibility() {
    let f = Fixture::new(false).await;
    let session = f.session.clone();
    let api = f.api.clone();
    let files = f.files().with_options(FileOptions {
        observer: Some(Arc::new(move |event, _| {
            if event == FileEvent::DestinationReady {
                session.adopt(api.clone(), json!({"userId":2}), &session.token()?)?;
            }
            Ok(())
        })),
        ..Default::default()
    });
    let op = f.start(&files);
    assert!(matches!(files.resume(&op.id), Err(Error::StaleResponse)));
    assert!(files.list().is_err());
    assert!(f.source.exists());
    assert!(
        CompletedDownloads::offline(f.store.clone(), "2")
            .unwrap()
            .get(&op.id)
            .is_err()
    );
    assert_eq!(
        f.offline().resume(&op.id).unwrap().phase,
        FilePhase::Complete
    );
    let f = Fixture::new(false).await;
    let _lock = OperationLock::acquire(
        f.store.directory(),
        "download-task",
        &serde_json::to_string(&("1", &f.task.id)).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        f.files().start(&f.task.id, FileAction::Forget, None),
        Err(Error::MutationBusy)
    ));
}

#[tokio::test]
async fn modified_sources_existing_targets_and_private_state_destinations_are_rejected() {
    let f = Fixture::new(false).await;
    let files = f.files();
    for path in [
        f.source.clone(),
        f.target().with_extension("mp3"),
        f.store.directory().join("audio.flac"),
    ] {
        assert!(
            files
                .start(&f.task.id, FileAction::Move, Some(path))
                .is_err()
        );
    }
    assert!(files.rename(&f.task.id, "../bad.flac").is_err());
    fs::create_dir_all(f.target().parent().unwrap()).unwrap();
    fs::write(f.target(), FLAC).unwrap();
    assert!(
        files
            .start(&f.task.id, FileAction::Move, Some(f.target()))
            .is_err()
    );
    fs::write(&f.source, b"changed").unwrap();
    assert!(files.start(&f.task.id, FileAction::Delete, None).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), b"changed");
    assert!(files.list().unwrap().is_empty());
}

#[tokio::test]
async fn database_commit_failure_rolls_back_all_indexes_before_source_removal() {
    let f = Fixture::new(true).await;
    let library = f.library();
    library.scan(f.source.parent().unwrap()).unwrap();
    let old = file_id(&f.source).unwrap();
    let files = f.files();
    let op = f.start(&files);
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_move BEFORE INSERT ON state WHEN NEW.kind='download_file_operations_v1' AND json_extract(NEW.value,'$.phase')='cleanup' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(files.resume(&op.id).is_err());
    assert_eq!(files.get(&op.id).unwrap().phase, FilePhase::Prepared);
    assert!(f.source.exists());
    assert_eq!(
        f.service.get(&f.task.id).unwrap().target,
        Some(f.source.clone())
    );
    assert!(library.item(&old).is_ok());
    assert!(library.item(&file_id(&f.target()).unwrap()).is_err());
    db.execute_batch("DROP TRIGGER fail_move;").unwrap();
    f.offline().resume(&op.id).unwrap();
    assert!(!f.source.exists());
}

#[tokio::test]
async fn committed_cleanup_retains_replacements_and_rejects_edits_to_the_original_inode() {
    let f = Fixture::new(false).await;
    let files = f
        .files()
        .with_options(interrupt(FileEvent::MetadataCommitted));
    let op = f.start(&files);
    assert!(files.resume(&op.id).is_err());
    fs::rename(&f.source, f.source.with_extension("kept")).unwrap();
    fs::write(&f.source, b"replacement").unwrap();
    let done = f.offline().resume(&op.id).unwrap();
    assert!(done.files[0].replacement_retained);
    assert_eq!(fs::read(&f.source).unwrap(), b"replacement");
    let f = Fixture::new(false).await;
    let mut options = interrupt(FileEvent::MetadataCommitted);
    options.copy_only = true;
    let files = f.files().with_options(options);
    let op = f.start(&files);
    assert!(files.resume(&op.id).is_err());
    fs::write(&f.source, b"edited old inode").unwrap();
    assert!(f.offline().resume(&op.id).is_err());
    assert_eq!(f.offline().get(&op.id).unwrap().phase, FilePhase::Cleanup);
    assert_eq!(fs::read(&f.source).unwrap(), b"edited old inode");
    assert_eq!(fs::read(f.target()).unwrap(), FLAC);
}

#[tokio::test]
async fn stale_local_revisions_and_cue_references_are_not_promoted_after_move() {
    let f = Fixture::new(false).await;
    let library = f.library();
    library.scan(f.source.parent().unwrap()).unwrap();
    let id = file_id(&f.source).unwrap();
    let mut stale = serde_json::to_value(library.resource(&id).unwrap()).unwrap();
    stale["local_revision"]["fingerprint"]["bytes"] = json!(99);
    fs::write(
        f.source.parent().unwrap().join("test.cue"),
        "FILE \"input.flac\" WAVE\n TRACK 01 AUDIO\n INDEX 01 00:00:00\n",
    )
    .unwrap();
    library.scan(f.source.parent().unwrap()).unwrap();
    let cue = library
        .list(Default::default())
        .unwrap()
        .into_iter()
        .find(|i| i.id != id)
        .unwrap();
    let cue_resource = serde_json::to_value(library.resource(&cue.id).unwrap()).unwrap();
    let mut q = PlaybackQueue::new(Some(f.store.clone()), "1").unwrap();
    q.replace(vec![stale, cue_resource, json!({"id":10})], 0, json!(null))
        .unwrap();
    let files = f.files();
    let op = f.start(&files);
    files.resume(&op.id).unwrap();
    q.reload().unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q.selected().unwrap().resource["id"], 10);
    assert!(library.item(&cue.id).is_err());
}

#[tokio::test]
async fn unjournaled_preexisting_task_schema_and_lost_file_acknowledgement_are_supported() {
    let f = Fixture::new(false).await;
    let value = f
        .store
        .get("1", "download_tasks_v1", &f.task.id)
        .unwrap()
        .unwrap();
    assert!(value.get("file_operation").is_none());
    let files = f.files();
    let op = f.start(&files);
    // Fail the ready checkpoint after the hardlink but before acknowledgement.
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_ready BEFORE INSERT ON state WHEN NEW.kind='download_file_operations_v1' AND json_extract(NEW.value,'$.files[0].stage')='ready' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(files.resume(&op.id).is_err());
    assert!(f.target().exists());
    assert!(f.source.exists());
    db.execute_batch("DROP TRIGGER fail_ready;").unwrap();
    f.offline().resume(&op.id).unwrap();
    assert!(!f.source.exists());
    assert_eq!(fs::read(f.target()).unwrap(), FLAC);
}

#[tokio::test]
async fn intent_failure_never_changes_files_or_leaves_a_task_marker() {
    let f = Fixture::new(false).await;
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_intent BEFORE INSERT ON state WHEN NEW.kind='download_file_operations_v1' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        f.files()
            .start(&f.task.id, FileAction::Move, Some(f.target()))
            .is_err()
    );
    assert!(f.service.get(&f.task.id).unwrap().file_operation.is_none());
    assert!(f.source.exists());
    assert!(!f.target().exists());
    assert!(f.files().list().unwrap().is_empty());
}

#[tokio::test]
async fn same_content_foreign_destination_is_neither_accepted_nor_deleted() {
    let f = Fixture::new(false).await;
    let files = f
        .files()
        .with_options(interrupt(FileEvent::DestinationReady));
    let op = f.start(&files);
    assert!(files.resume(&op.id).is_err());
    let retained = f.target().with_extension("retained");
    fs::rename(f.target(), &retained).unwrap();
    fs::write(f.target(), FLAC).unwrap();
    assert!(f.offline().resume(&op.id).is_err());
    assert!(f.offline().cancel(&op.id).is_err());
    assert!(f.source.exists());
    assert_eq!(fs::read(f.target()).unwrap(), FLAC);
    fs::remove_file(f.target()).unwrap();
    f.offline().cancel(&op.id).unwrap();
    assert!(retained.exists());
    assert!(f.source.exists());
}

#[tokio::test]
async fn replaced_source_parent_and_stale_scan_records_remain_detectable() {
    let f = Fixture::new(false).await;
    let files = f.files();
    let op = f.start(&files);
    let parent = f.source.parent().unwrap();
    let retained = parent.with_file_name("retained");
    fs::rename(parent, &retained).unwrap();
    fs::create_dir(parent).unwrap();
    fs::write(&f.source, FLAC).unwrap();
    assert!(files.resume(&op.id).is_err());
    assert!(files.cancel(&op.id).is_err());
    assert!(f.source.exists());
    assert!(retained.join("input.flac").exists());
    assert!(!f.target().exists());
    let f = Fixture::new(false).await;
    let library = f.library();
    library.scan(f.source.parent().unwrap()).unwrap();
    let id = file_id(&f.source).unwrap();
    library.map_online(&f.session, &id, &json!(123)).unwrap();
    let mut stale = f
        .store
        .get("local", "library_files_v1", &id)
        .unwrap()
        .unwrap();
    stale["revision"]["fingerprint"]["bytes"] = json!(1);
    f.store
        .put("local", "library_files_v1", &id, &stale)
        .unwrap();
    let op = f.start(&f.files());
    f.files().resume(&op.id).unwrap();
    assert!(
        f.store
            .values("local", "library_files_v1")
            .unwrap()
            .is_empty()
    );
    assert!(f.store.values("1", "local_map_v1").unwrap().is_empty());
    assert_eq!(fs::read(f.target()).unwrap(), FLAC);
}

#[tokio::test]
async fn rename_keeps_directory_format_and_sidecar_and_can_be_run_offline() {
    let f = Fixture::new(true).await;
    let files = f.offline();
    let op = files.rename(&f.task.id, "新名称.flac").unwrap();
    files.resume(&op.id).unwrap();
    let renamed = f.source.with_file_name("新名称.flac");
    assert_eq!(fs::read(&renamed).unwrap(), FLAC);
    assert!(renamed.with_extension("lrc").exists());
    assert!(!f.source.exists());
    assert_eq!(f.service.get(&f.task.id).unwrap().target, Some(renamed));
    assert!(files.rename(&f.task.id, "wrong.mp3").is_err());
}
