mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    download::{
        completed::{CompletedDownloads, FileAction, FilePhase},
        offline::OfflineLibrary,
        service::{
            DownloadDestination as Destination, DownloadOrigin, DownloadRunOptions as Run,
            DownloadService, DownloadStatus as Status, LyricsStatus,
        },
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{Arc, Mutex, atomic::Ordering},
    time::Duration,
};
const AUDIO: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
const LIST: &str = "/api/v1/cloud/get";
const DOWNLOAD: &str = "/api/cloud/dowonload";
#[tokio::test]
async fn cloud_suffix_matches_executed_official_javascript_including_unicode_whitespace() {
    let oracle: Value = serde_json::from_str(include_str!("fixtures/cloud_download.json")).unwrap();
    assert_eq!(oracle["wrapper"]["requests"][0], json!({"songId":"123"}));
    for case in oracle["cases"].as_array().unwrap() {
        let f = Fixture::new();
        f.api.resource.lock().unwrap()["name"] = case["name"].clone();
        *f.api.fallback.lock().unwrap() = json!({"code":200,"type":"flac"});
        let task = f.create(false).await;
        let result = f.service.run(&task.id, Run::default()).await;
        let official = case["suffix"].as_str().unwrap();
        let expected = if official.is_empty() {
            "flac"
        } else {
            official
        };
        // Official \w permits underscores; shared portable naming deliberately
        // refuses those and long extensions before creating media output.
        if expected.len() > 8 || !expected.bytes().all(|b| b.is_ascii_alphanumeric()) {
            assert!(matches!(result, Err(Error::InvalidInput(_))), "{case}");
            continue;
        }
        let task = result.unwrap();
        assert_eq!(task.status, Status::Complete);
        assert_eq!(
            task.resource.as_ref().unwrap()["type"],
            expected.to_ascii_lowercase(),
            "{case}"
        );
        assert_eq!(
            f.api.count("/api/song/enhance/download/url/v1"),
            usize::from(official.is_empty()),
            "{case}"
        );
    }
}
struct Gate {
    entered: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}
struct Server {
    rows: Mutex<Vec<Value>>,
    resource: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    gate: Mutex<Option<Arc<Gate>>>,
    fallback: Mutex<Value>,
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
                LIST => Ok(
                    json!({"code":200,"data":self.rows.lock().unwrap().clone(),"hasMore":false,"size":AUDIO.len(),"maxSize":1_000_000_000}),
                ),
                DOWNLOAD => {
                    assert_eq!(payload, json!({"songId":"123"}));
                    let gate = self.gate.lock().unwrap().take();
                    if let Some(g) = gate {
                        g.entered.notify_one();
                        g.release.acquire().await.unwrap().forget();
                    }
                    Ok(self.resource.lock().unwrap().clone())
                }
                "/api/song/enhance/download/url/v1" => {
                    Ok(json!({"code":200,"data":self.fallback.lock().unwrap().clone()}))
                }
                "/api/cloud/lyric/get" => {
                    assert_eq!(
                        payload,
                        json!({"songId":"123","userId":"1","lv":-1,"kv":-1})
                    );
                    Ok(json!({"code":200,"lrc":"[00:01]cloud original lyric\n"}))
                }
                _ => panic!("cloud workflow must not call {path}"),
            }
        })
    }
}
struct Fixture {
    root: tempfile::TempDir,
    api: Arc<Server>,
    media: Arc<support::download::Backend>,
    store: Arc<StateStore>,
    session: Arc<AccountSession>,
    service: DownloadService,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(Server {
            rows: Mutex::new(vec![
                json!({"songId":123,"fileName":"original.flac","fileSize":AUDIO.len(),"addTime":1,"simpleSong":{"id":987,"name":"Cloud original","ar":[{"name":"Artist"}],"al":{"name":"Album"},"dt":1000,"privilege":{"fee":1,"flag":8196}}}),
            ]),
            resource: Mutex::new(
                json!({"code":200,"name":" \\\"original.FLAC' ","size":AUDIO.len(),"url":"https://m.test.music.126.net/original?auth=fixture"}),
            ),
            calls: Mutex::new(vec![]),
            gate: Mutex::new(None),
            fallback: Mutex::new(json!({"code":404})),
        });
        let media = Arc::new(support::download::Backend::new(AUDIO.into()));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let service =
            DownloadService::with_backend(session.clone(), media.clone(), 8 * 1024 * 1024).unwrap();
        Self {
            root,
            api,
            media,
            store,
            session,
            service,
        }
    }
    fn destination(&self) -> Destination {
        Destination::Named {
            root: self.root.path().join("out"),
            path_type: 0,
            file_type: 1,
        }
    }
    async fn create(&self, lyrics: bool) -> cumulus_core::download::service::DownloadTask {
        self.service
            .create_cloud(&json!(123), self.destination(), lyrics)
            .await
            .unwrap()
    }
    fn reopen(&self) -> DownloadService {
        let store = Arc::new(StateStore::open(self.root.path().join("state")).unwrap());
        let session = Arc::new(AccountSession::new(self.api.clone(), Some(store)));
        session
            .adopt(
                self.api.clone(),
                json!({"userId":1}),
                &session.token().unwrap(),
            )
            .unwrap();
        DownloadService::with_backend(session, self.media.clone(), 8 * 1024 * 1024).unwrap()
    }
}

#[tokio::test]
async fn cloud_original_preserves_bytes_tags_and_origin_and_exports_cloud_lyrics_with_plain_offline_access()
 {
    let f = Fixture::new();
    let task = f.create(true).await;
    assert!(task.origin.is_cloud());
    assert!(!task.options.tags && !task.options.cover);
    let done = f.service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert!(!done.encrypted);
    assert_eq!(done.lyrics_status, LyricsStatus::Complete);
    assert!(!done.cleanup_pending);
    let path = done.target.as_ref().unwrap();
    assert_eq!(path.file_name().unwrap(), "Cloud original.flac");
    assert_eq!(fs::read(path).unwrap(), AUDIO);
    assert_eq!(
        fs::read_to_string(path.with_extension("lrc")).unwrap(),
        "[00:01]cloud original lyric\n"
    );
    assert!(done.resource.as_ref().unwrap()["md5"].is_null());
    assert!(done.resource.as_ref().unwrap()["level"].is_null());
    let offline = OfflineLibrary::new(f.session.clone())
        .unwrap()
        .check(&f.session.token().unwrap(), path, true)
        .await
        .unwrap();
    assert!(offline.access.can_play && offline.integrity_verified);
    assert_eq!(offline.record.song_id, "123");
    assert_eq!(f.reopen().get(&task.id).unwrap().origin, task.origin);
    assert_eq!(f.api.count(DOWNLOAD), 1);
}

#[tokio::test]
async fn paused_cloud_without_server_md5_reopens_and_resumes_at_the_persisted_byte_using_the_same_authorization()
 {
    let f = Fixture::new();
    let task = f.create(false).await;
    let paused = f
        .service
        .run(
            &task.id,
            Run {
                byte_budget: Some(117),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.status, Status::Paused);
    assert_eq!(paused.bytes, 117);
    let reopened = f.reopen();
    reopened.enqueue(&task.id).unwrap();
    let done = reopened.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), AUDIO);
    assert_eq!(f.api.count(DOWNLOAD), 1);
    let calls = f.media.audio.calls();
    assert_eq!(calls[1].offset, 117);
    assert_eq!(calls[1].if_range.as_deref(), Some("\"fixture-v1\""));
}

#[tokio::test]
async fn expired_authorization_renews_only_cloud_endpoint_once_and_restarts_when_new_bytes_cannot_be_proven_identical()
 {
    let f = Fixture::new();
    let task = f.create(false).await;
    f.service
        .run(
            &task.id,
            Run {
                byte_budget: Some(117),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    f.media.http_failures.store(1, Ordering::SeqCst);
    f.service.enqueue(&task.id).unwrap();
    f.api.resource.lock().unwrap()["url"] =
        json!("https://m.test.music.126.net/original?auth=renewed");
    let done = f.service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(f.api.count(DOWNLOAD), 2);
    assert_eq!(done.authorization_count, 2);
    assert_eq!(f.media.audio.calls()[1].offset, 0);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), AUDIO);
}

#[tokio::test]
async fn invalid_or_denied_original_authorizations_never_start_media_requests() {
    for (key, value) in [
        ("code", json!(403)),
        ("size", json!(1)),
        ("name", json!("no-extension")),
        ("name", json!("audio.flac/escape")),
        ("url", json!("https://example.com/secret")),
        ("md5", json!("bad")),
        ("songId", json!(999)),
        ("freeTrialInfo", json!({"start":0,"end":1000})),
    ] {
        let f = Fixture::new();
        let task = f.create(false).await;
        f.api.resource.lock().unwrap()[key] = value;
        assert!(
            f.service.run(&task.id, Run::default()).await.is_err(),
            "{key}"
        );
        assert!(f.media.audio.calls().is_empty());
        assert!(
            OfflineLibrary::new(f.session.clone())
                .unwrap()
                .records(&f.session.token().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn missing_or_changed_cloud_original_is_rejected_before_authorization_and_existing_outputs_are_preserved()
 {
    for changed in [false, true] {
        let f = Fixture::new();
        let task = f.create(false).await;
        if changed {
            f.api.rows.lock().unwrap()[0]["fileName"] = json!("changed.flac");
        } else {
            f.api.rows.lock().unwrap().clear();
        }
        assert!(f.service.run(&task.id, Run::default()).await.is_err());
        assert_eq!(f.api.count(DOWNLOAD), 0);
        assert!(f.media.audio.calls().is_empty());
    }
    let f = Fixture::new();
    let task = f.create(false).await;
    let path = f.root.path().join("out/Cloud original.flac");
    fs::write(&path, b"existing user file").unwrap();
    assert!(f.service.run(&task.id, Run::default()).await.is_err());
    assert_eq!(fs::read(path).unwrap(), b"existing user file");
    assert!(f.media.audio.calls().is_empty());
}

#[tokio::test]
async fn late_cloud_authorization_after_account_switch_does_not_create_transfer_or_publish_for_new_owner()
 {
    let f = Fixture::new();
    let task = f.create(false).await;
    let gate = Arc::new(Gate {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Semaphore::new(0),
    });
    *f.api.gate.lock().unwrap() = Some(gate.clone());
    let service = f.service.clone();
    let id = task.id.clone();
    let work = tokio::spawn(async move { service.run(&id, Run::default()).await });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    gate.release.add_permits(1);
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(f.media.audio.calls().is_empty());
    let other =
        DownloadService::with_backend(f.session.clone(), f.media.clone(), 8 * 1024 * 1024).unwrap();
    assert!(other.list().unwrap().is_empty());
}

#[tokio::test]
async fn publication_commit_failure_recovers_cloud_task_and_offline_record_without_redownloading() {
    let f = Fixture::new();
    let task = f.create(false).await;
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_complete BEFORE INSERT ON state WHEN NEW.kind='download_tasks_v1' AND json_extract(NEW.value,'$.status')='complete' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(
        f.service.run(&task.id, Run::default()).await,
        Err(Error::Storage(_))
    ));
    assert!(
        OfflineLibrary::new(f.session.clone())
            .unwrap()
            .records(&f.session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    let media_calls = f.media.audio.calls().len();
    db.execute_batch("DROP TRIGGER fail_complete;").unwrap();
    let done = f.reopen().run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert!(done.origin.is_cloud());
    assert_eq!(f.api.count(DOWNLOAD), 1);
    assert_eq!(f.media.audio.calls().len(), media_calls);
}

#[tokio::test]
async fn cloud_completed_rename_move_and_delete_keep_origin_and_ledger_consistent_without_remote_mutations()
 {
    let f = Fixture::new();
    let task = f.create(true).await;
    let done = f.service.run(&task.id, Run::default()).await.unwrap();
    let old = done.target.unwrap();
    let files = CompletedDownloads::new(f.session.clone()).unwrap();
    let rename = files.rename(&task.id, "renamed.flac").unwrap();
    assert_eq!(files.resume(&rename.id).unwrap().phase, FilePhase::Complete);
    assert!(!old.exists());
    let renamed = f.service.get(&task.id).unwrap();
    assert!(renamed.origin.is_cloud());
    assert_eq!(fs::read(renamed.target.as_ref().unwrap()).unwrap(), AUDIO);
    let target = f
        .root
        .path()
        .canonicalize()
        .unwrap()
        .join("elsewhere")
        .join("moved.flac");
    fs::create_dir(target.parent().unwrap()).unwrap();
    let moving = files
        .start(&task.id, FileAction::Move, Some(target.clone()))
        .unwrap();
    files.resume(&moving.id).unwrap();
    assert_eq!(
        f.service.get(&task.id).unwrap().target.as_ref(),
        Some(&target)
    );
    assert!(target.with_extension("lrc").is_file());
    let deletion = files.start(&task.id, FileAction::Delete, None).unwrap();
    files.resume(&deletion.id).unwrap();
    assert!(!target.exists());
    assert!(!target.with_extension("lrc").exists());
    assert!(
        OfflineLibrary::new(f.session.clone())
            .unwrap()
            .records(&f.session.token().unwrap())
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.api.count(DOWNLOAD), 1);
}

#[tokio::test]
async fn official_missing_suffix_fallback_uses_only_the_format_and_keeps_the_cloud_original_bytes_and_url()
 {
    let f = Fixture::new();
    let task = f.create(false).await;
    f.api.resource.lock().unwrap()["name"] = json!("original");
    *f.api.fallback.lock().unwrap() = json!({"code":200,"type":"flac","url":"https://m.test.music.126.net/different-catalog-file","size":999,"md5":"ffffffffffffffffffffffffffffffff","flag":8196,"fee":1,"level":"hires"});
    let done = f.service.run(&task.id, Run::default()).await.unwrap();
    assert_eq!(done.status, Status::Complete);
    assert_eq!(fs::read(done.target.unwrap()).unwrap(), AUDIO);
    assert!(!done.encrypted);
    assert_eq!(f.api.count("/api/song/enhance/download/url/v1"), 1);
    assert!(
        f.media
            .audio
            .calls()
            .iter()
            .all(|request| request.url.as_str().contains("/original?"))
    );
    assert!(done.resource.unwrap()["level"].is_null());
}

#[tokio::test]
async fn legacy_song_tasks_without_origin_deserialize_as_songs() {
    let f = Fixture::new();
    let task = f
        .service
        .create(&json!(123), f.destination(), Default::default())
        .unwrap();
    let mut value = serde_json::to_value(&task).unwrap();
    value.as_object_mut().unwrap().remove("origin");
    let restored: cumulus_core::download::service::DownloadTask =
        serde_json::from_value(value).unwrap();
    assert_eq!(restored.origin, DownloadOrigin::Song);
}

#[tokio::test]
async fn client_cloud_download_selection_checks_fresh_revision_and_entire_ownership_before_creating_any_task()
 {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, dto::DownloadCommand},
        cloud::library::CloudLibrary,
        protocol::http::ApiOrigin,
    };
    let f = Fixture::new();
    let client = MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(f.store.directory(), ApiOrigin::Pc).unwrap(),
    )
    .with_download_backend(f.media.clone());
    client
        .download_command(
            "1",
            DownloadCommand::Configure {
                root: f.root.path().join("out").to_str().unwrap().into(),
                path_type: 0,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    let snapshot = CloudLibrary::new(f.session.clone())
        .unwrap()
        .refresh()
        .await
        .unwrap();
    assert!(matches!(
        client
            .download_command(
                "1",
                DownloadCommand::StartCloud {
                    song_ids: vec!["123".into()],
                    revision: "old".into(),
                    lyrics: false
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(
        client
            .download_command(
                "1",
                DownloadCommand::StartCloud {
                    song_ids: vec!["123".into(), "999".into()],
                    revision: snapshot.revision.clone(),
                    lyrics: false
                }
            )
            .await
            .is_err()
    );
    assert!(matches!(
        client
            .download_command(
                "old",
                DownloadCommand::StartCloud {
                    song_ids: vec!["123".into()],
                    revision: snapshot.revision,
                    lyrics: false
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.service.list().unwrap().is_empty());
    assert_eq!(f.api.count(DOWNLOAD), 0);
    assert!(f.media.audio.calls().is_empty());
}

#[tokio::test]
async fn native_client_schedules_deduplicated_cloud_selection_and_projects_original_source_without_urls_or_fabricated_quality()
 {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, dto::*},
        cloud::library::CloudLibrary,
        protocol::http::ApiOrigin,
    };
    let f = Fixture::new();
    let client = MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(f.store.directory(), ApiOrigin::Pc).unwrap(),
    )
    .with_download_backend(f.media.clone());
    client
        .download_command(
            "1",
            DownloadCommand::Configure {
                root: f.root.path().join("out").to_str().unwrap().into(),
                path_type: 0,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    let snapshot = CloudLibrary::new(f.session.clone())
        .unwrap()
        .refresh()
        .await
        .unwrap();
    let accepted = client
        .download_command(
            "1",
            DownloadCommand::StartCloud {
                song_ids: vec!["123".into(), "123".into()],
                revision: snapshot.revision,
                lyrics: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(accepted.accepted.len(), 1);
    assert!(accepted.failed.is_empty());
    let done = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = client
                .downloads(
                    "1",
                    DownloadQuery {
                        tab: DownloadTab::Completed,
                        category: Default::default(),
                        keyword: String::new(),
                        sort: DownloadSort::Completed,
                        descending: true,
                        offset: 0,
                        limit: 100,
                    },
                )
                .await
                .unwrap();
            if page.completed_count == 1 {
                break page;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(done.tasks[0].source, "cloud");
    assert!(done.tasks[0].actual_quality.is_none());
    assert!(!done.tasks[0].encrypted);
    assert!(
        !serde_json::to_string(&done)
            .unwrap()
            .contains("auth=fixture")
    );
    assert_eq!(f.api.count(DOWNLOAD), 1);
    assert_eq!(
        fs::read(done.tasks[0].path.as_ref().unwrap()).unwrap(),
        AUDIO
    );
}
