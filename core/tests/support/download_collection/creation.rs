use super::*;
use cumulus_core::{
    client::dto::{DownloadCollectCreation, DownloadCollectCreationCommand as Action},
    protocol::api::{GuardedPlaylistCreate, SystemLikedRead},
};

const OWNED: &str = "/api/user/playlist";
const CREATE: &str = "/api/playlist/create";
const NAME: &str = "合成下载收藏";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Success,
    PreflightUnavailable,
    LostCreate,
    LateList,
    LostAdd,
    Foreign,
    ExistingId,
    CancelBeforeCreate,
    CancelBeforeAdd,
}

struct Native {
    case: Case,
    writes: Mutex<Vec<(&'static str, Value)>>,
    reads: Mutex<Vec<String>>,
    show_created: AtomicBool,
    show_added: AtomicBool,
    credential_rotated: AtomicBool,
    liked: super::liked::Liked,
    hold_liked_read: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl Native {
    fn new(case: Case) -> Self {
        Self {
            case,
            writes: Mutex::default(),
            reads: Mutex::default(),
            show_created: AtomicBool::new(!matches!(case, Case::LostCreate | Case::LateList)),
            show_added: AtomicBool::new(false),
            credential_rotated: AtomicBool::new(false),
            liked: Default::default(),
            hold_liked_read: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        }
    }

    fn row(&self) -> Value {
        let mut row = list("91", 1, 0);
        row["name"] = json!(NAME);
        row
    }
}

impl Api for Native {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.reads.lock().unwrap().push(path.into());
            let writes = self.writes.lock().unwrap().len();
            match path {
                OWNED => {
                    assert_eq!(
                        payload,
                        json!({"uid":"1","includeVideo":true,"offset":0,"limit":200})
                    );
                    if self.case == Case::PreflightUnavailable
                        && self
                            .reads
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|p| *p == OWNED)
                            .count()
                            == 1
                    {
                        return Err(Error::Transport("synthetic failed preflight read".into()));
                    }
                    let mut rows = vec![list("81", 1, 0)];
                    if self.case == Case::ExistingId
                        || writes > 0 && self.show_created.load(Ordering::Acquire)
                    {
                        rows.push(self.row());
                    }
                    Ok(json!({"code":200,"more":false,"playlist":rows}))
                }
                DETAIL => {
                    assert_eq!(payload, json!({"id":"91","n":100000,"s":0}));
                    let mut row = self.row();
                    if self.case == Case::Foreign {
                        row["creator"]["userId"] = json!(2);
                    }
                    row["trackIds"] = if self.show_added.load(Ordering::Acquire) {
                        json!([{"id":"123","v":1}])
                    } else {
                        json!([])
                    };
                    Ok(json!({"code":200,"playlist":row}))
                }
                _ => panic!("unexpected ordinary download creation request {path}"),
            }
        })
    }

    fn download_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some("synthetic-download-account".into()))
    }

    fn download_collection_create_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some(
            if self.credential_rotated.load(Ordering::Acquire) {
                "another-synthetic-credential"
            } else {
                "synthetic-download-account"
            }
            .into(),
        ))
    }

    fn download_system_liked_scope_revision(&self) -> Result<Option<String>> {
        self.liked.download_system_liked_scope_revision()
    }

    fn request_download_system_liked<'a>(
        &'a self,
        read: SystemLikedRead,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let result = self
                .liked
                .request_download_system_liked(read, revision, permission)
                .await?;
            if self.hold_liked_read.swap(false, Ordering::AcqRel) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(result)
        })
    }

    fn request_guarded_playlist_create<'a>(
        &'a self,
        write: GuardedPlaylistCreate,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, body) = write.into_request();
            assert_eq!(path, CREATE);
            assert_eq!(body, json!({"uid":"1","name":NAME,"privacy":10}));
            if self.case == Case::CancelBeforeCreate {
                self.entered.notify_one();
                self.release.notified().await;
            }
            permission()?;
            dispatched.store(true, Ordering::Release);
            let mut writes = self.writes.lock().unwrap();
            assert!(writes.is_empty(), "do not replay a dispatched create");
            writes.push((path, body));
            if self.case == Case::LostCreate {
                return Err(Error::Transport("synthetic lost create ACK".into()));
            }
            Ok(json!({"code":200,"id":"91"}))
        })
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (path, body) = write.into_request()?;
            assert_eq!(path, WRITE);
            if body["pid"] == "83" {
                permission()?;
                dispatched.store(true, Ordering::Release);
                return self.liked.apply(body);
            }
            assert_eq!(body, json!({"pid":"91","trackIds":"[\"123\"]","op":"add"}));
            if self.case == Case::CancelBeforeAdd {
                self.entered.notify_one();
                self.release.notified().await;
            }
            permission()?;
            dispatched.store(true, Ordering::Release);
            let mut writes = self.writes.lock().unwrap();
            assert_eq!(writes.len(), 1, "do not replay or preempt the add");
            writes.push((path, body));
            if self.case == Case::LostAdd {
                return Err(Error::Transport("synthetic lost add ACK".into()));
            }
            self.show_added.store(true, Ordering::Release);
            Ok(json!({"code":200}))
        })
    }
}

async fn fixture(case: Case) -> (Fixture, Arc<Native>, String, String, String) {
    let f = Fixture::new();
    let (task, _) = f.completed().await;
    let api = Arc::new(Native::new(case));
    f.session
        .adopt(
            api.clone(),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let epoch = f.client.account().unwrap().epoch;
    let plan = f
        .client
        .download_collect_prepare(&epoch, &task)
        .unwrap()
        .value;
    assert!(plan.can_create);
    (f, api, epoch, plan.receipt, task)
}

fn create() -> Action {
    Action::Create {
        name: NAME.into(),
        private_playlist: true,
    }
}

fn status(reconcile: bool) -> Action {
    Action::Status { reconcile }
}

fn finished(result: &DownloadCollectCreation) {
    assert_eq!(result.create.phase, PlaylistPhase::Confirmed);
    assert_eq!(result.create.playlist_id.as_deref(), Some("91"));
    let add = result.add.as_ref().unwrap();
    assert_eq!(add.phase, PlaylistPhase::Confirmed);
    assert_eq!(add.observed.as_ref().unwrap().track_ids, ["123"]);
    assert!(!result.can_add);
}

#[tokio::test]
async fn explicit_create_journals_the_exact_new_destination_and_one_add() {
    let (f, api, epoch, receipt, _) = fixture(Case::Success).await;
    assert!(api.writes.lock().unwrap().is_empty());
    assert!(
        f.client
            .download_collect_creation(&epoch, &receipt, Action::AddCreated {})
            .await
            .is_err()
    );
    let result = f
        .client
        .download_collect_creation(&epoch, &receipt, create())
        .await
        .unwrap()
        .value;
    finished(&result);
    assert_eq!(result.receipt, receipt);
    for action in [create(), Action::AddCreated {}, status(true)] {
        let same = f
            .client
            .download_collect_creation(&epoch, &receipt, action)
            .await
            .unwrap()
            .value;
        finished(&same);
        assert_eq!(same.create.operation_id, result.create.operation_id);
        assert_eq!(
            same.add.unwrap().operation_id,
            result.add.as_ref().unwrap().operation_id
        );
    }
    assert_eq!(*api.reads.lock().unwrap(), [OWNED, OWNED, DETAIL, DETAIL]);
    assert_eq!(
        api.writes
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| *p)
            .collect::<Vec<_>>(),
        [CREATE, WRITE]
    );
}

#[tokio::test]
async fn a_failed_preflight_creates_no_journal_and_can_be_explicitly_retried() {
    let (f, api, epoch, receipt, _) = fixture(Case::PreflightUnavailable).await;
    let error = f
        .client
        .download_collect_creation(&epoch, &receipt, create())
        .await
        .err()
        .expect("offline preflight must not create");
    assert!(matches!(
        error,
        Error::State("download collection create not sent")
    ));
    assert!(api.writes.lock().unwrap().is_empty());
    assert!(
        f.client
            .download_collect_creation(&epoch, &receipt, status(false))
            .await
            .is_err()
    );
    let retry = f
        .client
        .download_collect_creation(&epoch, &receipt, create())
        .await
        .unwrap();
    finished(&retry.value);
    assert_eq!(api.writes.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn lost_create_may_be_read_back_but_never_proves_add_authority() {
    let (f, api, epoch, receipt, _) = fixture(Case::LostCreate).await;
    let initial = f
        .client
        .download_collect_creation(&epoch, &receipt, create())
        .await
        .unwrap()
        .value;
    assert_eq!(initial.create.phase, PlaylistPhase::Uncertain);
    assert!(!initial.can_add && initial.add.is_none());
    api.show_created.store(true, Ordering::Release);
    let local = f
        .client
        .download_collect_creation(&epoch, &receipt, status(false))
        .await
        .unwrap()
        .value;
    assert_eq!(local.create.phase, PlaylistPhase::Uncertain);
    let recovered = f
        .client
        .download_collect_creation(&epoch, &receipt, status(true))
        .await
        .unwrap()
        .value;
    assert_eq!(recovered.create.operation_id, initial.create.operation_id);
    assert_eq!(recovered.create.phase, PlaylistPhase::Confirmed);
    assert_eq!(recovered.create.playlist_id.as_deref(), Some("91"));
    assert!(!recovered.can_add && recovered.add.is_none());
    assert!(
        f.client
            .download_collect_creation(&epoch, &receipt, Action::AddCreated {})
            .await
            .is_err()
    );
    assert!(
        f.client
            .download_collect_existing(&epoch, &receipt, "91")
            .await
            .is_err()
    );
    assert_eq!(*api.reads.lock().unwrap(), [OWNED, OWNED, OWNED]);
    assert_eq!(api.writes.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn acknowledged_create_can_explicitly_resume_a_never_sent_add_but_lost_add_cannot_replay() {
    for case in [Case::LateList, Case::LostAdd] {
        let (f, api, epoch, receipt, _) = fixture(case).await;
        let initial = f
            .client
            .download_collect_creation(&epoch, &receipt, create())
            .await
            .unwrap()
            .value;
        assert!(!initial.can_add);
        if case == Case::LateList {
            assert!(initial.add.is_none());
            api.show_created.store(true, Ordering::Release);
            let settled = f
                .client
                .download_collect_creation(&epoch, &receipt, status(true))
                .await
                .unwrap()
                .value;
            assert!(settled.can_add && settled.add.is_none());
            assert_eq!(api.writes.lock().unwrap().len(), 1);
            finished(
                &f.client
                    .download_collect_creation(&epoch, &receipt, Action::AddCreated {})
                    .await
                    .unwrap()
                    .value,
            );
        } else {
            assert_eq!(initial.add.unwrap().phase, PlaylistPhase::Uncertain);
            let still = f
                .client
                .download_collect_creation(&epoch, &receipt, Action::AddCreated {})
                .await
                .unwrap()
                .value;
            assert_eq!(still.add.unwrap().phase, PlaylistPhase::Uncertain);
            api.show_added.store(true, Ordering::Release);
            finished(
                &f.client
                    .download_collect_creation(&epoch, &receipt, status(true))
                    .await
                    .unwrap()
                    .value,
            );
        }
        assert_eq!(api.writes.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn recycled_identity_conflicting_owner_and_stale_credentials_cannot_add() {
    for case in [Case::Foreign, Case::ExistingId] {
        let (f, api, epoch, receipt, _) = fixture(case).await;
        let result = f
            .client
            .download_collect_creation(&epoch, &receipt, create())
            .await;
        if case == Case::Foreign {
            assert!(matches!(result, Err(Error::StaleResponse)));
        } else {
            let result = result.unwrap().value;
            assert!(!result.can_add && result.add.is_none());
        }
        assert_eq!(api.writes.lock().unwrap().len(), 1);
        api.credential_rotated.store(true, Ordering::Release);
        assert!(matches!(
            f.client
                .download_collect_creation(&epoch, &receipt, Action::AddCreated {})
                .await,
            Err(Error::StaleResponse)
        ));
        assert_eq!(api.writes.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn closing_or_removing_the_source_fences_create_and_add_at_their_dispatches() {
    for (case, retire_task) in [
        (Case::CancelBeforeCreate, false),
        (Case::CancelBeforeCreate, true),
        (Case::CancelBeforeAdd, false),
        (Case::CancelBeforeAdd, true),
    ] {
        let (f, api, epoch, receipt, task) = fixture(case).await;
        let (result, ()) = tokio::join!(
            f.client
                .download_collect_creation(&epoch, &receipt, create()),
            async {
                tokio::time::timeout(std::time::Duration::from_secs(2), api.entered.notified())
                    .await
                    .unwrap();
                if retire_task {
                    f.store.delete("1", "download_tasks_v1", &task).unwrap();
                } else {
                    f.client.download_collect_cancel(&epoch, &receipt).unwrap();
                }
                api.release.notify_one();
            }
        );
        assert!(result.is_err());
        let sent = api
            .writes
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| *p)
            .collect::<Vec<_>>();
        if case == Case::CancelBeforeAdd {
            assert_eq!(sent, [CREATE]);
        } else {
            assert!(sent.is_empty());
        }
        assert!(
            f.client
                .download_collect_creation(&epoch, &receipt, Action::AddCreated {})
                .await
                .is_err()
        );
    }
}

#[test]
fn http_create_requires_the_same_url_scoped_cookie_as_download_add_and_account() {
    use cumulus_core::{
        account::credentials::Credentials,
        protocol::http::{
            ApiOrigin, ClientProfile, HttpBackend, HttpReply, MusicHttpClient, RequestOptions,
            RequestPlan, RoutingPolicy, WireRoute,
        },
        storage::StateStore,
    };

    struct NoWire;
    impl HttpBackend for NoWire {
        fn post(&self, _: RequestPlan) -> ApiFuture<'_, HttpReply> {
            Box::pin(async { panic!("credential scope proof is local only") })
        }
    }

    for (routing, route) in [
        (RoutingPolicy::OfficialPcPlaylists, WireRoute::Eapi),
        (RoutingPolicy::VerifiedCompatibility, WireRoute::Weapi),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let origin = ApiOrigin::Pc;
        let credentials = Arc::new(
            Credentials::open(Arc::new(StateStore::open(dir.path()).unwrap()), origin).unwrap(),
        );
        let set = |url: &url::Url, cookie: String| {
            let revision = credentials.request_credentials(url).unwrap().revision;
            credentials
                .record_response(url, &[cookie], None, &revision)
                .unwrap();
        };
        set(
            &origin.url(),
            "MUSIC_U=synthetic-account; Secure; Path=/".into(),
        );
        let api = MusicHttpClient::with_backend(
            credentials.clone(),
            ClientProfile::default(),
            RequestOptions {
                routing,
                ..Default::default()
            },
            None,
            Arc::new(NoWire),
        )
        .unwrap();
        let existing = api.download_collection_scope_revision().unwrap().unwrap();
        assert_eq!(
            api.download_collection_create_scope_revision()
                .unwrap()
                .as_deref(),
            Some(existing.as_str())
        );
        let create = origin.request_url(route, CREATE).unwrap();
        set(
            &create,
            format!("MUSIC_U=synthetic-conflict; Secure; Path={}", create.path()),
        );
        assert!(api.download_collection_scope_revision().unwrap().is_some());
        assert!(matches!(
            api.download_collection_create_scope_revision(),
            Err(Error::CredentialChanged)
        ));
    }
}

#[tokio::test]
async fn pending_creation_rejects_previously_resolved_liked_destination_without_a_second_write() {
    let (f, api, epoch, visit, _) = fixture(Case::LostCreate).await;
    let liked = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap()
        .receipt;
    let pending = f
        .client
        .download_collect_creation(&epoch, &visit, create())
        .await
        .unwrap()
        .value;
    assert_eq!(pending.create.phase, PlaylistPhase::Uncertain);
    assert!(pending.add.is_none());
    let discovery_reads = api.liked.reads.lock().unwrap().len();
    assert!(matches!(
        f.client
            .download_collect_liked_destination(&epoch, &visit)
            .await,
        Err(Error::MutationBusy)
    ));
    for reconcile in [false, true] {
        assert!(matches!(
            f.client
                .download_collect_liked(&epoch, &visit, &liked, reconcile)
                .await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(api.liked.reads.lock().unwrap().len(), discovery_reads);
    assert!(api.liked.writes.lock().unwrap().is_empty());
    let original = f
        .client
        .download_collect_creation(&epoch, &visit, status(false))
        .await
        .unwrap()
        .value;
    assert_eq!(original.create.operation_id, pending.create.operation_id);
    assert_eq!(api.writes.lock().unwrap().len(), 1);
    assert_eq!(f.client.playlist_operations(&epoch).unwrap().value.len(), 1);
}

#[tokio::test]
async fn pending_liked_attempt_rejects_creation_and_keeps_its_read_only_reconciliation() {
    let (f, api, epoch, visit, _) = fixture(Case::Success).await;
    let liked = f
        .client
        .download_collect_liked_destination(&epoch, &visit)
        .await
        .unwrap()
        .value
        .unwrap()
        .receipt;
    let pending = f
        .client
        .download_collect_liked(&epoch, &visit, &liked, false)
        .await
        .unwrap()
        .value;
    assert_eq!(pending.phase, PlaylistPhase::Uncertain);
    assert_eq!(pending.verified_liked_track_ids, Some(vec![]));
    for command in [create(), status(true), Action::AddCreated {}] {
        assert!(matches!(
            f.client
                .download_collect_creation(&epoch, &visit, command)
                .await,
            Err(Error::StaleResponse)
        ));
    }
    assert!(api.reads.lock().unwrap().is_empty());
    assert!(api.writes.lock().unwrap().is_empty());
    let operations = f.client.playlist_operations(&epoch).unwrap().value;
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].operation_id, pending.operation_id);
    api.liked.state.store(2, Ordering::Release);
    let confirmed = f
        .client
        .download_collect_liked(&epoch, &visit, &liked, true)
        .await
        .unwrap()
        .value;
    assert_eq!(confirmed.operation_id, pending.operation_id);
    assert_eq!(confirmed.phase, PlaylistPhase::Confirmed);
    assert_eq!(confirmed.verified_liked_track_ids, Some(vec!["123".into()]));
    assert_eq!(api.liked.writes.lock().unwrap().len(), 1);
    assert!(
        f.client
            .playlist_operations(&epoch)
            .unwrap()
            .value
            .is_empty()
    );
}

#[tokio::test]
async fn liked_discovery_returning_after_creation_cannot_publish_a_destination() {
    let (f, api, epoch, visit, _) = fixture(Case::LostCreate).await;
    api.hold_liked_read.store(true, Ordering::Release);
    let (discovery, pending) = tokio::join!(
        f.client.download_collect_liked_destination(&epoch, &visit),
        async {
            tokio::time::timeout(std::time::Duration::from_secs(2), api.entered.notified())
                .await
                .unwrap();
            let pending = f
                .client
                .download_collect_creation(&epoch, &visit, create())
                .await;
            api.release.notify_one();
            pending.unwrap().value
        }
    );
    assert_eq!(pending.create.phase, PlaylistPhase::Uncertain);
    assert!(matches!(discovery, Err(Error::StaleResponse)));
    assert_eq!(api.liked.reads.lock().unwrap().len(), 1);
    assert!(api.liked.writes.lock().unwrap().is_empty());
    assert_eq!(api.writes.lock().unwrap().len(), 1);
}
