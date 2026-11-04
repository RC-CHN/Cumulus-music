#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback;
mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken},
    download::{
        offline::OfflineLibrary,
        service::{
            DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadService,
            DownloadStatus, OutputContainer,
        },
    },
    playback::{
        PlaybackMode, PlaybackQueue,
        controller::{PlaybackCommand, PlaybackController},
        preflight::{OrdinaryCandidate as C, OrdinaryQueueEnvironment as E, Undetermined},
        source::SourceResolver,
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use rand::{SeedableRng, rngs::StdRng};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
    },
    time::Duration,
};

const ONLINE: E = E::OnlineWithoutMvExperiment { app_online: true };
const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");

struct ApiFixture {
    download: support::download::Server,
    detail: Mutex<Vec<Value>>,
    code: AtomicU16,
    member: AtomicBool,
    member_reads: AtomicUsize,
    gate: Mutex<Option<Arc<support::download::PlaybackGate>>>,
}

impl Api for ApiFixture {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        match path {
            "/api/song/enhance/download/url/v1" => self.download.request(path, body, cache),
            "/api/music-vip-membership/client/vip/info" => Box::pin(async move {
                self.member_reads.fetch_add(1, Ordering::SeqCst);
                let associator = if self.member.load(Ordering::SeqCst) {
                    json!({"expireTime":9999999999999u64,"vipLevel":7})
                } else {
                    Value::Null
                };
                Ok(json!({"code":200,"data":{"now":2000000,"userId":1,"associator":associator}}))
            }),
            "/api/v3/song/detail" if body.get("trialMode").is_none() => {
                assert_eq!(body, json!({"c":"[{\"id\":\"123\",\"v\":0}]"}));
                self.download.request(path, body, cache)
            }
            "/api/v3/song/detail" => Box::pin(async move {
                assert!(!cache);
                assert_eq!(body.as_object().unwrap().len(), 2);
                assert_eq!(body["trialMode"], -1);
                let packed: Vec<Value> = serde_json::from_str(body["c"].as_str().unwrap()).unwrap();
                assert!(!packed.is_empty() && packed.len() <= 500);
                assert!(
                    packed
                        .iter()
                        .all(|row| row["id"].is_string() && row["v"] == 0)
                );
                self.detail.lock().unwrap().push(json!(packed));
                let gate = self.gate.lock().unwrap().clone();
                if let Some(gate) = gate {
                    gate.entered.notify_one();
                    gate.release.notified().await;
                }
                let code = self.code.load(Ordering::SeqCst);
                let ids: Vec<_> = packed.iter().filter_map(|v| v["id"].as_str()).collect();
                let songs: Vec<_> = ids
                    .contains(&"10")
                    .then(|| json!({"id":10}))
                    .into_iter()
                    .collect();
                let privileges: Vec<_> = if ids.contains(&"20") {
                    // More privileges than songs adds the official placeholder.
                    vec![json!({"id":10,"pl":128000}), json!({"id":20,"pl":0})]
                } else {
                    songs
                        .iter()
                        .map(|r| json!({"id":r["id"],"pl":128000}))
                        .collect()
                };
                Ok(json!({"code":code,"songs":songs,"privileges":privileges}))
            }),
            _ => panic!("preflight must not call {path}"),
        }
    }
}

struct Fixture {
    root: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ApiFixture>,
    session: Arc<AccountSession>,
    device: Arc<playback::Device>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let api = Arc::new(ApiFixture {
            download: support::download::Server::new(FLAC, "flac"),
            detail: Mutex::new(vec![]),
            code: AtomicU16::new(200),
            member: AtomicBool::new(true),
            member_reads: AtomicUsize::new(0),
            gate: Mutex::new(None),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        Self {
            root,
            store,
            api,
            session,
            device: Arc::new(playback::Device::default()),
        }
    }

    fn token(&self) -> SessionToken {
        self.session.token().unwrap()
    }

    fn player(&self, rows: Vec<Value>) -> Arc<PlaybackController> {
        let mut queue =
            PlaybackQueue::with_rng(Some(self.store.clone()), "1", StdRng::seed_from_u64(7))
                .unwrap();
        queue.replace(rows, 0, Value::Null).unwrap();
        queue.set_mode(PlaybackMode::Random).unwrap();
        Arc::new(
            PlaybackController::start(
                self.session.clone(),
                self.store.clone(),
                Arc::new(SourceResolver::new(self.session.clone(), None)),
                playback::audio_engine(self.device.clone()),
            )
            .unwrap(),
        )
    }

    async fn publish(&self, name: &str, encrypted: bool) -> std::path::PathBuf {
        let backend = Arc::new(support::download::Backend::new(FLAC.into()));
        let service =
            DownloadService::with_backend(self.session.clone(), backend, 1 << 20).unwrap();
        let target = self.root.path().join(name);
        {
            let mut response = self.api.download.resource.lock().unwrap();
            response["flag"] = json!(0);
            response["fee"] = if encrypted { json!(1) } else { json!(0) };
        }
        self.api.download.song.lock().unwrap()["privilege"]["flag"] = json!(4);
        let task = service
            .create(
                &json!(123),
                DownloadDestination::Path { path: target },
                DownloadOptions {
                    tags: false,
                    cover: false,
                    container: if encrypted {
                        OutputContainer::Ncm
                    } else {
                        OutputContainer::Audio
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        let task = service
            .run(&task.id, DownloadRunOptions::default())
            .await
            .unwrap();
        assert_eq!(task.status, DownloadStatus::Complete);
        assert_eq!(task.encrypted, encrypted);
        task.target.unwrap()
    }
}

fn song(id: usize, privilege: Value, backup: Value) -> Value {
    json!({"id":id,"song":{"id":id,"privilege":privilege,"noCopyrightRcmd":backup}})
}

fn denied(id: usize, alternative: usize) -> Value {
    song(
        id,
        json!({"pl":0,"flag":0}),
        json!({"type":1,"songId":alternative}),
    )
}

#[tokio::test]
async fn reads_display_order_download_before_online_then_same_recording_and_first_final_id() {
    let f = Fixture::new();
    f.publish("ready.flac", false).await;
    let player = f.player(vec![
        denied(123, 77),
        denied(1, 10),
        denied(2, 10),
        denied(3, 20),
        denied(4, 404),
        song(10, json!({"pl":128000}), Value::Null),
        song(5, json!({"pl":0,"flag":1024}), Value::Null),
        song(6, json!({"pl":0}), json!({"type":3,"mvInfo":{"id":10}})),
    ]);
    let token = f.token();
    let (before, queue) = player.queue_with_state(&token).await.unwrap();
    assert_ne!(
        queue.order,
        queue
            .items
            .iter()
            .map(|row| row.entry_id.clone())
            .collect::<Vec<_>>()
    );
    let plan = player
        .ordinary_queue_preflight(&token, before.queue_revision, ONLINE)
        .await
        .unwrap();
    let id = |id: &str| C::SameRecording { id: id.to_owned() };
    assert_eq!(
        plan.rows
            .iter()
            .map(|row| row.candidate.clone())
            .collect::<Vec<_>>(),
        vec![
            C::AccountDownload,
            id("10"),
            id("10"),
            id("20"),
            C::OriginalRetained {
                missing_alternative: "404".into()
            },
            C::Online,
            C::Online,
            C::Unavailable,
        ]
    );
    assert_eq!(plan.candidate_order, [0, 1, 3, 4, 6]);
    assert_eq!(plan.rows[1].entry_id, queue.items[1].entry_id);
    assert!(plan.fully_classified());
    assert_eq!(
        *f.api.detail.lock().unwrap(),
        vec![json!([
            {"id":"10","v":0},{"id":"10","v":0},{"id":"20","v":0},{"id":"404","v":0}
        ])]
    );
    let (after, reread) = player.queue_with_state(&token).await.unwrap();
    assert_eq!(reread, queue);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn download_corruption_is_unknown_and_known_missing_file_can_fall_through() {
    let f = Fixture::new();
    let target = f.publish("ready.flac", false).await;
    let player = f.player(vec![denied(123, 10)]);
    let token = f.token();
    let revision = player.snapshot().queue_revision;
    fs::write(&target, b"corrupt replacement").unwrap();
    let corrupt = player
        .ordinary_queue_preflight(&token, revision, ONLINE)
        .await
        .unwrap();
    assert_eq!(
        corrupt.rows[0].candidate,
        C::Undetermined(Undetermined::DownloadPublication)
    );
    assert!(f.api.detail.lock().unwrap().is_empty());
    fs::remove_file(&target).unwrap();
    let missing = player
        .ordinary_queue_preflight(&token, revision, ONLINE)
        .await
        .unwrap();
    assert_eq!(
        missing.rows[0].candidate,
        C::SameRecording { id: "10".into() }
    );
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn encrypted_download_requires_existing_entitlement_and_never_renews_from_preflight() {
    let f = Fixture::new();
    let target = f.publish("locked.ncm", true).await;
    assert_eq!(target.extension().unwrap(), "ncm");
    let player = f.player(vec![denied(123, 10)]);
    let token = f.token();
    let revision = player.snapshot().queue_revision;
    assert_eq!(f.api.member_reads.load(Ordering::SeqCst), 1);
    f.store.delete("1", "membership", "current").unwrap();
    let unknown = player
        .ordinary_queue_preflight(&token, revision, ONLINE)
        .await
        .unwrap();
    assert_eq!(
        unknown.rows[0].candidate,
        C::Undetermined(Undetermined::DownloadPublication)
    );
    assert_eq!(f.api.member_reads.load(Ordering::SeqCst), 1);
    let membership = OfflineLibrary::new(f.session.clone()).unwrap();
    f.api.member.store(false, Ordering::SeqCst);
    membership.refresh_membership(&token).await.unwrap();
    let without = player
        .ordinary_queue_preflight(&token, revision, ONLINE)
        .await
        .unwrap();
    assert_eq!(
        without.rows[0].candidate,
        C::SameRecording { id: "10".into() }
    );
    assert_eq!(f.api.member_reads.load(Ordering::SeqCst), 2);
    f.api.member.store(true, Ordering::SeqCst);
    membership.refresh_membership(&token).await.unwrap();
    let with = player
        .ordinary_queue_preflight(&token, revision, ONLINE)
        .await
        .unwrap();
    assert_eq!(with.rows[0].candidate, C::AccountDownload);
    assert_eq!(f.api.member_reads.load(Ordering::SeqCst), 3);
    assert_eq!(f.api.detail.lock().unwrap().len(), 1);
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
}

#[tokio::test]
async fn does_not_infer_environment_or_skip_over_missing_catalog_metadata() {
    let f = Fixture::new();
    let player = f.player(vec![
        denied(1, 10),
        json!({"id":"2","missing":true}),
        denied(3, 20),
    ]);
    let token = f.token();
    let revision = player.snapshot().queue_revision;
    let outcomes = [
        (E::Unresolved, C::Undetermined(Undetermined::Environment)),
        (
            E::OnlineWithoutMvExperiment { app_online: false },
            C::Unavailable,
        ),
    ];
    for (environment, first) in outcomes {
        let plan = player
            .ordinary_queue_preflight(&token, revision, environment)
            .await
            .unwrap();
        assert_eq!(plan.rows[0].candidate, first);
        assert_eq!(
            plan.rows[1].candidate,
            C::Undetermined(Undetermined::CatalogMetadataMissing)
        );
        assert_eq!(plan.rows[2].candidate, C::NotScanned);
        assert!(!plan.fully_classified());
        assert!(plan.candidate_order.is_empty());
    }
    assert!(f.api.detail.lock().unwrap().is_empty());
    assert!(matches!(
        player
            .ordinary_queue_preflight(&token, revision + 1, ONLINE)
            .await,
        Err(Error::QueueChanged)
    ));
    let explicit_local = f.player(vec![json!({"id":"1","local_id":"abc"})]);
    assert!(matches!(
        explicit_local
            .ordinary_queue_preflight(&token, explicit_local.snapshot().queue_revision, ONLINE)
            .await,
        Err(Error::InvalidInput(_))
    ));
}

#[tokio::test]
async fn detail_failure_is_not_song_denial_and_stale_queue_or_account_cannot_publish() {
    for account_switch in [false, true] {
        let f = Fixture::new();
        let player = f.player(vec![denied(1, 10), denied(2, 20)]);
        let token = f.token();
        let (before, queue) = player.queue_with_state(&token).await.unwrap();
        for code in [403, 503] {
            f.api.code.store(code, Ordering::SeqCst);
            let err = player
                .ordinary_queue_preflight(&token, before.queue_revision, ONLINE)
                .await
                .unwrap_err();
            assert!(err.is_service_code(code.into()));
            assert_eq!(player.queue(&token).await.unwrap(), queue);
        }
        f.api.code.store(200, Ordering::SeqCst);
        let gate = Arc::new(support::download::PlaybackGate::default());
        *f.api.gate.lock().unwrap() = Some(gate.clone());
        let pending = {
            let player = player.clone();
            let token = token.clone();
            tokio::spawn(async move {
                player
                    .ordinary_queue_preflight(&token, before.queue_revision, ONLINE)
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
            .await
            .unwrap();
        if account_switch {
            f.session
                .adopt(f.api.clone(), json!({"userId":2}), &token)
                .unwrap();
        } else {
            let reversed = queue
                .items
                .iter()
                .rev()
                .map(|entry| entry.entry_id.clone())
                .collect();
            player
                .command(&token, PlaybackCommand::Reorder(reversed))
                .await
                .unwrap();
        }
        gate.release.notify_one();
        let error = tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(if account_switch {
            matches!(error, Error::StaleResponse)
        } else {
            matches!(error, Error::QueueChanged)
        });
        assert_eq!(f.device.state.lock().unwrap().loads, 0);
    }
}

#[tokio::test]
async fn keeps_duplicate_backup_ids_across_the_official_five_hundred_item_batches() {
    let f = Fixture::new();
    let player = f.player((1000..1501).map(|id| denied(id, 10)).collect());
    let token = f.token();
    let result = player
        .ordinary_queue_preflight(&token, player.snapshot().queue_revision, ONLINE)
        .await
        .unwrap();
    let calls = f.api.detail.lock().unwrap();
    let mut lengths: Vec<_> = calls.iter().map(|c| c.as_array().unwrap().len()).collect();
    lengths.sort_unstable();
    assert_eq!(lengths, [1, 500]);
    assert!(
        calls
            .iter()
            .flat_map(|c| c.as_array().unwrap())
            .all(|row| row["id"] == "10")
    );
    assert_eq!(result.rows.len(), 501);
    assert_eq!(result.candidate_order, [0]);
    assert!(result.fully_classified());
}

#[tokio::test]
async fn a_concurrent_download_publication_invalidates_the_older_online_evidence() {
    let f = Fixture::new();
    let player = f.player(vec![denied(123, 10)]);
    let token = f.token();
    let gate = Arc::new(support::download::PlaybackGate::default());
    *f.api.gate.lock().unwrap() = Some(gate.clone());
    let pending = {
        let player = player.clone();
        let token = token.clone();
        tokio::spawn(async move {
            player
                .ordinary_queue_preflight(&token, player.snapshot().queue_revision, ONLINE)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    f.publish("while-detail-pending.flac", false).await;
    gate.release.notify_one();
    let error = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, Error::StaleResponse));
    let fresh = player
        .ordinary_queue_preflight(&token, player.snapshot().queue_revision, ONLINE)
        .await
        .unwrap();
    assert_eq!(fresh.rows[0].candidate, C::AccountDownload);
    assert_eq!(f.api.detail.lock().unwrap().len(), 1);
}
