use cumulus_core::{
    Error, Result,
    account::AccountSession,
    library::{
        AudioMetadata, LibraryItem, ListOptions, LocalLibrary, MappingSource, catalog::LocalQuery,
        matching::*, probe::AudioProbe,
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};
use tokio::sync::Notify;

struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("wav".into()),
            container: "wav".into(),
            duration: 12.125,
            title: path.file_stem().unwrap().to_string_lossy().into_owned(),
            album: "专辑".into(),
            artists: vec!["歌手 & A".into()],
            bitrate: None,
            sample_rate: None,
            channels: None,
            cover_bytes: None,
        })
    }
}
struct Hints;
impl MetadataProbe for Hints {
    fn inspect(&self, item: &LibraryItem) -> Result<MatchHint> {
        Ok(match item.metadata.title.as_str() {
            "hint" => MatchHint::Song(json!({"id":"987","name":"元信息歌曲"})),
            "invalid" => MatchHint::Invalid,
            _ => MatchHint::Absent,
        })
    }
}
#[derive(Default)]
struct Echo {
    batches: Mutex<Vec<Vec<MatchQuery>>>,
    gate: AtomicBool,
    mode: AtomicU8,
    started: Notify,
    release: Notify,
}
impl Echo {
    async fn wait(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(3), self.started.notified())
            .await
            .unwrap();
    }
}
impl Api for Echo {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            if path == "/api/v3/song/detail" {
                self.started.notify_one();
                if self.gate.swap(false, Ordering::SeqCst) {
                    self.release.notified().await;
                }
                return Ok(json!({"code":200,"songs":[{"id":555,"name":"手动选择"}]}));
            }
            assert_eq!(path, "/api/search/match/new");
            let queries: Vec<MatchQuery> =
                serde_json::from_str(payload["songs"].as_str().unwrap()).unwrap();
            assert!(queries.len() <= 50);
            self.batches.lock().unwrap().push(queries.clone());
            self.started.notify_one();
            if self.gate.swap(false, Ordering::SeqCst) {
                self.release.notified().await;
            }
            match self.mode.swap(0, Ordering::SeqCst) {
                1 => return Ok(json!({"code":403})),
                2 => {
                    return Ok(
                        json!({"code":200,"result":{"ids":["foreign?bad"],"songs":[{"id":1}]}}),
                    );
                }
                _ => (),
            }
            Ok(json!({"code":200,"result":{
                "ids":queries.iter().map(|q| &q.persist_id).collect::<Vec<_>>(),
                "songs":queries.iter().map(|q| json!({"id":123,"name":q.title,"playTime":12125,
                    "djProgramId":if q.title=="djresult" {1} else {0}})).collect::<Vec<_>>()}}))
        })
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    media: PathBuf,
    store: Arc<StateStore>,
    library: LocalLibrary,
    session: Arc<AccountSession>,
    api: Arc<Echo>,
    service: LocalMatching,
}
impl Fixture {
    fn new(names: &[String]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir(&media).unwrap();
        for name in names {
            std::fs::write(media.join(format!("{name}.wav")), "fixture").unwrap();
        }
        let store = Arc::new(StateStore::open(dir.path().join("state")).unwrap());
        let library = LocalLibrary::with_probe(store.clone(), Arc::new(Probe)).unwrap();
        library.scan(&media).unwrap();
        let api = Arc::new(Echo::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let service =
            LocalMatching::with_probe(session.clone(), library.clone(), Arc::new(Hints)).unwrap();
        Self {
            _dir: dir,
            media,
            store,
            library,
            session,
            api,
            service,
        }
    }
    fn reopen(&self) -> LocalMatching {
        LocalMatching::with_probe(self.session.clone(), self.library.clone(), Arc::new(Hints))
            .unwrap()
    }
    fn items(&self) -> Vec<LibraryItem> {
        self.library.list(ListOptions::default()).unwrap()
    }
    fn start(&self) -> MatchJob {
        self.service.start(&LocalQuery::default()).unwrap()
    }
    fn spawn(&self) -> tokio::task::JoinHandle<Result<Option<MatchJob>>> {
        let service = self.service.clone();
        tokio::spawn(async move { service.step().await })
    }
}
fn names(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("Song {i:03}")).collect()
}

#[tokio::test]
async fn complete_selection_batches_resume_and_account_isolation() {
    let f = Fixture::new(&names(103));
    let job = f
        .service
        .start(&LocalQuery {
            limit: 1,
            offset: 100,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(job.total, 103);
    let j = f.service.step().await.unwrap().unwrap();
    assert_eq!((j.processed, j.matched), (50, 50));
    let reopened = f.reopen();
    let j = reopened.recover().unwrap().unwrap();
    assert_eq!(j.phase, MatchPhase::Paused);
    assert!(reopened.step().await.unwrap().is_none());
    reopened.control(&j.id, MatchAction::Resume).unwrap();
    assert_eq!(reopened.step().await.unwrap().unwrap().processed, 100);
    let done = reopened.step().await.unwrap().unwrap();
    assert_eq!((done.phase, done.matched), (MatchPhase::Complete, 103));
    assert_eq!(
        f.api
            .batches
            .lock()
            .unwrap()
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        vec![50, 50, 3]
    );
    assert_eq!(f.start().total, 0); // Current mappings are not automatically overwritten.
    let ids = f.items().iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    assert_eq!(
        f.library
            .online_mappings(&f.session, &f.session.token().unwrap(), &ids)
            .unwrap()
            .len(),
        103
    );
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(reopened.state(), Err(Error::StaleResponse)));
    assert!(f.reopen().state().unwrap().is_none());
    assert!(
        f.library
            .online_mappings(&f.session, &f.session.token().unwrap(), &ids)
            .unwrap()
            .is_empty()
    );
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":9}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.library
            .online_mappings(&f.session, &f.session.token().unwrap(), &ids)
            .unwrap()
            .len(),
        103
    );
}

#[tokio::test]
async fn metadata_precedes_network_and_invalid_tags_and_dj_do_not_fuzzy_match() {
    let f = Fixture::new(&["hint", "invalid", "plain", "djresult"].map(String::from));
    f.start();
    let done = f.service.step().await.unwrap().unwrap();
    assert_eq!((done.matched, done.unmatched, done.skipped), (2, 2, 0));
    let batches = f.api.batches.lock().unwrap();
    assert_eq!(batches[0].len(), 2);
    assert!(
        batches[0]
            .iter()
            .all(|q| !["hint", "invalid"].contains(&q.title.as_str()))
    );
    for item in f.items() {
        let map = f
            .library
            .online_mappings(
                &f.session,
                &f.session.token().unwrap(),
                std::slice::from_ref(&item.id),
            )
            .unwrap();
        match item.metadata.title.as_str() {
            "hint" => assert_eq!(map[&item.id].source, Some(MappingSource::FileMetadata)),
            "plain" => {
                assert_eq!(map[&item.id].song.as_ref().unwrap()["duration"], 12125);
                assert_eq!(map[&item.id].source, Some(MappingSource::Search));
            }
            _ => assert!(map.is_empty()),
        }
    }
}

#[tokio::test]
async fn changed_file_and_rescan_discard_late_match() {
    for rescan in [false, true] {
        let f = Fixture::new(&names(1));
        let old = f.items().remove(0);
        f.start();
        f.api.gate.store(true, Ordering::SeqCst);
        let task = f.spawn();
        f.api.wait().await;
        std::fs::write(&old.path, "replaced audio").unwrap();
        if rescan {
            f.library.scan(&f.media).unwrap();
        }
        f.api.release.notify_one();
        let done = task.await.unwrap().unwrap().unwrap();
        assert_eq!((done.matched, done.skipped), (0, 1));
        assert!(f.library.online_id(&f.session, &old.id).unwrap().is_none());
    }
}

#[tokio::test]
async fn index_scan_contention_waits_to_commit_without_repeating_search_and_can_cancel() {
    for cancel in [false, true] {
        let f = Fixture::new(&names(1));
        let job = f.start();
        f.api.gate.store(true, Ordering::SeqCst);
        let task = f.spawn();
        f.api.wait().await;
        let index = cumulus_core::storage::OperationLock::acquire(
            f.store.directory(),
            "local",
            "library-index",
        )
        .unwrap();
        f.api.release.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(
            !task.is_finished(),
            "index contention must not fail a matching job"
        );
        assert_eq!(
            f.service.state().unwrap().unwrap().phase,
            MatchPhase::Running
        );
        if cancel {
            f.service.control(&job.id, MatchAction::Cancel).unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(result, Err(Error::Cancelled)));
            drop(index);
            assert!(f.store.values("9", "local_map_v1").unwrap().is_empty());
        } else {
            drop(index);
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(result.phase, MatchPhase::Complete);
            assert_eq!(result.matched, 1);
        }
        assert_eq!(f.api.batches.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn pause_cancel_and_account_switch_fence_inflight_requests() {
    for action in [MatchAction::Pause, MatchAction::Cancel] {
        let f = Fixture::new(&names(1));
        let job = f.start();
        f.api.gate.store(true, Ordering::SeqCst);
        let task = f.spawn();
        f.api.wait().await;
        assert!(matches!(f.service.step().await, Err(Error::MutationBusy)));
        f.service.control(&job.id, action).unwrap();
        f.api.release.notify_one();
        assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
        assert_eq!(f.service.state().unwrap().unwrap().processed, 0);
        assert!(f.store.values("9", "local_map_v1").unwrap().is_empty());
    }
    let f = Fixture::new(&names(1));
    f.start();
    f.api.gate.store(true, Ordering::SeqCst);
    let task = f.spawn();
    f.api.wait().await;
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(f.reopen().state().unwrap().is_none());
    assert!(f.store.values("9", "local_map_v1").unwrap().is_empty());
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":9}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.reopen().recover().unwrap().unwrap().phase,
        MatchPhase::Paused
    );
}

#[tokio::test]
async fn service_and_foreign_response_failures_are_durable_and_explicitly_retryable() {
    for mode in [1, 2] {
        let f = Fixture::new(&names(2));
        let job = f.start();
        f.api.mode.store(mode, Ordering::SeqCst);
        assert!(f.service.step().await.is_err());
        let failed = f.service.state().unwrap().unwrap();
        assert_eq!(
            (failed.phase, failed.processed, failed.matched),
            (MatchPhase::Failed, 0, 0)
        );
        assert!(f.reopen().step().await.unwrap().is_none());
        f.reopen().control(&job.id, MatchAction::Resume).unwrap();
        assert_eq!(f.service.step().await.unwrap().unwrap().matched, 2);
    }
}

#[tokio::test]
async fn cursor_and_mapping_transaction_roll_back_together() {
    let f = Fixture::new(&names(2));
    let job = f.start();
    let db = rusqlite::Connection::open(f.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_mapping BEFORE INSERT ON state WHEN NEW.kind='local_map_v1' BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(f.service.step().await.is_err());
    assert!(f.store.values("9", "local_map_v1").unwrap().is_empty());
    assert_eq!(f.service.state().unwrap().unwrap().processed, 0);
    assert_eq!(
        f.store
            .values("9", "local_match_candidates_v1")
            .unwrap()
            .len(),
        2
    );
    db.execute_batch("DROP TRIGGER fail_mapping").unwrap();
    f.service.control(&job.id, MatchAction::Resume).unwrap();
    assert_eq!(f.service.step().await.unwrap().unwrap().matched, 2);
    assert!(
        f.store
            .values("9", "local_match_candidates_v1")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn manual_binding_clear_and_legacy_mapping_keep_revision_and_account_boundaries() {
    let f = Fixture::new(&names(1));
    let item = f.items().remove(0);
    let map = f
        .service
        .bind(&item.id, &item.revision, &json!(555))
        .await
        .unwrap();
    assert_eq!(map.song_id, "555");
    assert_eq!(
        f.library
            .online_id(&f.session, &item.id)
            .unwrap()
            .as_deref(),
        Some("555")
    );
    f.service.clear(&item.id, &item.revision).unwrap();
    assert!(f.library.online_id(&f.session, &item.id).unwrap().is_none());
    f.store
        .put(
            "9",
            "local_map_v1",
            &item.id,
            &json!({"song_id":"777","revision":item.revision}),
        )
        .unwrap();
    assert_eq!(
        f.library
            .online_id(&f.session, &item.id)
            .unwrap()
            .as_deref(),
        Some("777")
    );
    std::fs::write(&item.path, "changed source").unwrap();
    f.library.scan(&f.media).unwrap();
    assert!(matches!(
        f.service.bind(&item.id, &item.revision, &json!(555)).await,
        Err(Error::QueueChanged)
    ));
    assert!(f.library.online_id(&f.session, &item.id).unwrap().is_none());
}

#[test]
fn protocol_encoding_metadata_and_hostile_identity_boundaries() {
    let f = Fixture::new(&["unicode fixture".into()]);
    let mut item = f.items().remove(0);
    // Metadata may contain characters forbidden in Windows filenames.
    item.metadata.title = "雪 !'()*~ +%?".into();
    item.id = "local-1".into();
    let q = MatchQuery::from_item(&item).unwrap();
    assert_eq!(
        q.persist_id,
        "local-1?%E9%9B%AA%20!'()*~%20%2B%25%3F-%E6%AD%8C%E6%89%8B%20%26%20A-12.125"
    );
    for result in [
        json!({"ids":[q.persist_id,q.persist_id],"songs":[{"id":1},{"id":2}]}),
        json!({"ids":["local-1?different"],"songs":[{"id":1}]}),
        json!({"ids":[q.persist_id],"songs":[{"id":0}]}),
    ] {
        assert!(normalize_matches(&result, std::slice::from_ref(&q)).is_err());
    }
    let song=decode_metadata("music:{\"musicId\":\"18446744073709551616\",\"musicName\":\"歌曲\",\"artist\":[[\"作者\",7]],\"bitrate\":900000,\"flag\":4,\"privilege\":{\"flag\":260}}").unwrap();
    assert_eq!(song["id"], "18446744073709551616");
    assert_eq!(song["bitrate"], 999000.0);
    assert_eq!(song["privilege"]["flag"], 260);
    assert!(decode_metadata("music:garbage").is_none());
}

#[test]
fn file_comments_and_ncm_metadata_are_read_without_modifying_sources() {
    use cumulus_core::media::{
        ncm::{NcmWriteOptions, write_file},
        tags::{track_plan, write_tagged_copy},
    };
    use sha2::{Digest, Sha256};
    let f = Fixture::new(&[]);
    let plan=track_plan(&json!({"id":123,"name":"元信息歌曲","al":{"id":1,"name":"专辑"},"ar":[{"id":2,"name":"歌手"}],"dt":12125}),
        &json!({"type":"flac","md5":"ab".repeat(16),"br":128000}),false).unwrap();
    for (ext, seed) in [
        (
            "mp3",
            include_bytes!("fixtures/tag_audio/seed.mp3").as_slice(),
        ),
        (
            "flac",
            include_bytes!("fixtures/tag_audio/seed.flac").as_slice(),
        ),
        (
            "m4a",
            include_bytes!("fixtures/tag_audio/seed.m4a").as_slice(),
        ),
    ] {
        let source = f._dir.path().join(format!("seed.{ext}"));
        std::fs::write(&source, seed).unwrap();
        write_tagged_copy(&source, f.media.join(format!("tagged.{ext}")), &plan, None).unwrap();
    }
    let source = f.media.join("tagged.flac");
    let metadata = json!({"musicId":123,"musicName":"元信息歌曲"});
    write_file(
        &source,
        f.media.join("tagged.ncm"),
        NcmWriteOptions {
            metadata: Some(&metadata),
            ..Default::default()
        },
    )
    .unwrap();
    f.library.scan(&f.media).unwrap();
    for mut item in f.items() {
        item.metadata.container = item.path.extension().unwrap().to_str().unwrap().into();
        let before = Sha256::digest(std::fs::read(&item.path).unwrap());
        let MatchHint::Song(song) = FileMetadataProbe.inspect(&item).unwrap() else {
            panic!("missing metadata for {}", item.metadata.container)
        };
        assert_eq!(song["id"], "123");
        assert_eq!(song["name"], "元信息歌曲");
        assert_eq!(before, Sha256::digest(std::fs::read(&item.path).unwrap()));
    }
}

#[test]
fn matching_fields_and_metadata_agree_with_executed_official_helpers() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/local-matching-official.json")).unwrap();
    let f = Fixture::new(&names(1));
    let mut item = f.items().remove(0);
    for case in fixture["queries"].as_array().unwrap() {
        let input = &case["input"];
        item.id = input["id"].as_str().unwrap().into();
        item.metadata.title = input["name"].as_str().unwrap().into();
        item.metadata.duration = input["duration"].as_f64().unwrap() / 1000.0;
        assert_eq!(
            numeric_json(serde_json::to_value(MatchQuery::from_item(&item).unwrap()).unwrap()),
            case["expected"]
        );
    }
    for case in fixture["results"].as_array().unwrap() {
        let mut query = MatchQuery::from_item(&item).unwrap();
        query.persist_id = "local-1?opaque".into();
        assert_eq!(
            serde_json::to_value(normalize_matches(&case["input"], &[query]).unwrap()).unwrap(),
            case["expected"]
        );
    }
    for case in fixture["metadata"].as_array().unwrap() {
        assert_eq!(
            numeric_json(decode_metadata(case["input"].as_str().unwrap()).unwrap()),
            case["expected"],
            "{}",
            case["input"]
        );
    }
}

fn numeric_json(value: Value) -> Value {
    serde_json::from_str(&cumulus_core::protocol::wire::serialization::js_json(&value).unwrap())
        .unwrap()
}

#[tokio::test]
async fn concurrent_manual_mapping_and_account_switch_cannot_overwrite_new_state() {
    let f = Fixture::new(&names(1));
    let item = f.items().remove(0);
    f.api.gate.store(true, Ordering::SeqCst);
    let service = f.service.clone();
    let captured = item.clone();
    let pending = tokio::spawn(async move {
        service
            .bind(&captured.id, &captured.revision, &json!(555))
            .await
    });
    f.api.wait().await;
    assert!(matches!(
        f.reopen().clear(&item.id, &item.revision),
        Err(Error::MutationBusy)
    ));
    assert!(matches!(
        f.reopen().bind(&item.id, &item.revision, &json!(555)).await,
        Err(Error::MutationBusy)
    ));
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(f.store.values("9", "local_map_v1").unwrap().is_empty());
    assert!(f.store.values("10", "local_map_v1").unwrap().is_empty());
}
