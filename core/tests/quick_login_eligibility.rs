mod support;

use cumulus_core::{
    Error,
    account::{
        AccountSession,
        credential_import::LegacySession,
        credentials::Credentials,
        profiles::{ProfileRegistry, QuickLoginTransport, ServerQuickLogin, ServerQuickLoginPhase},
    },
    client::{
        MusicClient,
        dto::{LoginMode, ServerQuickLoginCommand},
    },
    protocol::http::{ApiOrigin, BodyStatus, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{fs, sync::Arc};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

const PATH: &str = "/api/login/getquickloginlist";

struct World {
    root: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    guest: Arc<MusicHttpClient>,
    backend: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
    ids: Vec<String>,
}

impl World {
    async fn new(count: u32, replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(&root.path().join("registry"), ApiOrigin::Pc).unwrap();
        let identity = (1..=count).map(|uid| {
            json!({"code":200,"profile":{
            "userId":uid,"nickname":format!("Saved {uid}"),
            "avatarUrl":format!("https://p1.music.126.net/{uid}.jpg?hidden=synthetic-avatar-query"),
            "avatarImgId":9007199254740995_u64,"userType":4,"cellphone":"13800138000",
            "unused":"synthetic-private-profile-field"
        }}).into()
        });
        let backend = Arc::new(ScriptHttp::new(identity.chain(replies)));
        let credentials = Arc::new(
            Credentials::open(
                Arc::new(StateStore::open(root.path().join("guest")).unwrap()),
                ApiOrigin::Pc,
            )
            .unwrap(),
        );
        let guest = Arc::new(
            MusicHttpClient::with_backend(
                credentials,
                Default::default(),
                Default::default(),
                None,
                backend.clone(),
            )
            .unwrap(),
        );
        let session = Arc::new(AccountSession::new(guest.clone(), Some(registry.store())));
        let mut ids = Vec::new();
        let import = root.path().join("synthetic-input");
        fs::create_dir(&import).unwrap();
        fs::write(
            import.join("device.json"),
            r#"{"deviceId":"synthetic-quick-device"}"#,
        )
        .unwrap();
        for uid in 1..=count {
            fs::write(import.join("cookies.txt"), format!(
                "# Netscape HTTP Cookie File\ninterfacepc.music.163.com\tFALSE\t/\tTRUE\t\tMUSIC_U\thistory-token-{uid}\n"
            )).unwrap();
            let candidate = registry
                .candidate_with_backend(
                    Default::default(),
                    Default::default(),
                    None,
                    backend.clone(),
                )
                .unwrap();
            candidate
                .credentials()
                .import_legacy(LegacySession::read(&import, ApiOrigin::Pc).unwrap())
                .unwrap();
            candidate
                .verify_and_adopt(&session, &session.token().unwrap())
                .await
                .unwrap();
            ids.push(registry.selection().unwrap().unwrap().session_id);
        }
        let client = Arc::new(
            MusicClient::with_session(session.clone(), registry.clone())
                .with_login_providers(Some(backend.clone()), None),
        );
        Self {
            root,
            registry,
            session,
            guest,
            backend,
            client,
            ids,
        }
    }

    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }

    async fn guest_attempt(&self, index: usize) -> ServerQuickLogin {
        self.session.attach(self.guest.clone()).unwrap();
        self.registry
            .begin_guest_server_quick_for(
                self.session.clone(),
                &self.session.token().unwrap(),
                &self.ids[index],
                QuickLoginTransport {
                    profile: Default::default(),
                    options: Default::default(),
                    context: None,
                    backend: self.backend.clone(),
                },
            )
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn exact_bounded_jsk_history_and_safe_projection_leave_profiles_untouched() {
    let mut response =
        reply(json!({"code":200,"deleteUserIds":[1],"vipInfo":{"1":{"secret":"poison-vip"}}}));
    response
        .set_cookies
        .push("MUSIC_U=poison-current; Path=/; Secure".into());
    response.refresh_token = Some("poison-refresh".into());
    let world = World::new(5, [HttpResponse::Ready(Ok(response))]).await;
    let before = world.registry.selection().unwrap();
    let result = world
        .client
        .quick_login_eligibility(&world.epoch())
        .await
        .unwrap();
    assert_eq!(result.epoch, world.epoch());
    assert_eq!(result.value.checked_count, 3);
    let calls = world.backend.calls();
    assert_eq!(calls.len(), 6);
    let call = &calls[5];
    assert_eq!(call.path, PATH);
    assert_eq!(call.payload["fromPage"], "JSK");
    assert_eq!(call.payload["currentEnv"], "{}");
    assert!(
        call.cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=history-token-5")
    );
    let history: Vec<Value> =
        serde_json::from_str(call.payload["userInfoList"].as_str().unwrap()).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["userId"], "5");
    assert_eq!(history[1]["userId"], "1");
    assert_eq!(history[2]["userId"], "2");
    assert_eq!(
        history[0],
        json!({
            "userId":"5","musicU":"history-token-5","userProfile":{
                "userId":"5","nickname":"Saved 5","avatarUrl":"https://p1.music.126.net/5.jpg",
                "avatarImgId":9007199254740995_u64,"userType":4,"cellphone":"13800138000"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(&result.value).unwrap(),
        json!({
            "checkedCount":3,"accounts":[
                {"id":world.ids[4],"current":true,"eligible":true},
                {"id":world.ids[0],"current":false,"eligible":false},
                {"id":world.ids[1],"current":false,"eligible":true}
            ]
        })
    );
    assert_eq!(world.registry.selection().unwrap(), before);
    assert_eq!(world.registry.saved().unwrap().len(), 5);
    let selected = world.registry.current(Default::default()).unwrap().unwrap();
    let credentials = selected.credentials.login_credentials().unwrap();
    assert_eq!(credentials.token.as_deref(), Some("history-token-5"));
    assert!(credentials.refresh_token.is_none());
}

#[tokio::test]
async fn missing_store_is_not_created_and_guest_with_no_history_sends_nothing() {
    let world = World::new(2, [json!({"code":200}).into()]).await;
    world.session.attach(world.guest.clone()).unwrap();
    let missing = world
        .root
        .path()
        .join("registry/sessions")
        .join(&world.ids[0])
        .join("core.sqlite3");
    fs::remove_file(&missing).unwrap();
    let result = world
        .client
        .quick_login_eligibility(&world.epoch())
        .await
        .unwrap();
    assert!(!missing.exists());
    assert_eq!(result.value.checked_count, 1);
    assert_eq!(result.value.accounts[0].id, world.ids[1]);
    assert!(!result.value.accounts[0].current);
    assert!(result.value.accounts[0].eligible);
    let empty = World::new(0, []).await;
    let view = empty
        .client
        .quick_login_eligibility(&empty.epoch())
        .await
        .unwrap()
        .value;
    assert!(view.accounts.is_empty());
    assert_eq!(view.checked_count, 0);
    assert!(empty.backend.calls().is_empty());
}

#[tokio::test]
async fn malformed_or_foreign_invalidations_do_not_delete_or_expose_history() {
    let world = World::new(
        2,
        [
            json!({"code":200,"deleteUserIds":[999],"secret":"poison-echo"}).into(),
            json!({"code":200,"deleteUserIds":{"bad":"poison-echo"}}).into(),
            json!({"code":301,"secret":"poison-echo"}).into(),
            json!({"code":200,"deleteUserIds":[1,1]}).into(),
            json!({"code":200,"usableUserIds":[2]}).into(),
        ],
    )
    .await;
    let epoch = world.epoch();
    for _ in 0..5 {
        let error = world
            .client
            .quick_login_eligibility(&epoch)
            .await
            .err()
            .expect("eligibility should fail");
        assert!(matches!(error, Error::Response(_)));
        assert!(!format!("{error:?}").contains("poison-echo"));
    }
    assert_eq!(world.registry.saved().unwrap().len(), 2);
    assert_eq!(world.epoch(), epoch);
    assert_eq!(world.session.token().unwrap().uid.as_deref(), Some("2"));
}

#[tokio::test]
async fn switching_account_cancels_inflight_eligibility_read() {
    let (pending, release) = gate();
    let world = World::new(2, [pending]).await;
    let client = world.client.clone();
    let epoch = world.epoch();
    let operation = tokio::spawn(async move { client.quick_login_eligibility(&epoch).await });
    world.backend.wait(3).await;
    world.session.attach(world.guest.clone()).unwrap();
    assert!(matches!(
        operation.await.unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(release.send(Ok(reply(json!({"code":200})))).is_err());
    assert_eq!(world.registry.saved().unwrap().len(), 2);
}

fn issued_quick_reply() -> cumulus_core::protocol::http::HttpReply {
    let mut response = reply(json!({"code":200,"profile":{"userId":1}}));
    response
        .set_cookies
        .push("MUSIC_U=fresh-target; Path=/; Secure".into());
    response.refresh_token = Some("fresh-target-rt".into());
    response
}

#[tokio::test]
async fn guest_server_quick_uses_fresh_credentials_and_verified_target_cas() {
    let world = World::new(
        2,
        [
            json!({"code":200,"deleteUserIds":[2]}).into(),
            HttpResponse::Ready(Ok(issued_quick_reply())),
            json!({"code":200,"profile":{"userId":1,"nickname":"Verified target"}}).into(),
        ],
    )
    .await;
    let old_path = world
        .root
        .path()
        .join("registry/sessions")
        .join(&world.ids[0]);
    let old_store = Arc::new(StateStore::open(old_path).unwrap());
    let old_credentials = Credentials::open(old_store, ApiOrigin::Pc).unwrap();
    let old_revision = old_credentials.login_credentials().unwrap().revision;
    let attempt = world.guest_attempt(0).await;
    assert_eq!(
        attempt.snapshot().unwrap().phase,
        ServerQuickLoginPhase::Ready
    );
    assert_eq!(
        attempt.execute().await.unwrap().phase,
        ServerQuickLoginPhase::Authenticated
    );
    assert_eq!(
        attempt.execute().await.unwrap().phase,
        ServerQuickLoginPhase::Authenticated
    );
    let calls = world.backend.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/w/nuser/account/get",
            "/api/w/nuser/account/get",
            PATH,
            "/api/login/quicklogin",
            "/api/w/nuser/account/get",
        ]
    );
    assert_eq!(calls[3].payload["fromPage"], "JSK");
    assert_eq!(calls[3].payload["currentEnv"], "{}");
    // The SDK supplies a Number; the PC EAPI form adapter encodes scalars as strings.
    assert_eq!(calls[3].payload["targetUserId"], "1");
    assert_eq!(calls[3].payload["currentUserId"], "");
    assert!(
        !calls[3]
            .cookie
            .as_deref()
            .unwrap_or("")
            .contains("MUSIC_U=")
    );
    let history: Vec<Value> =
        serde_json::from_str(calls[3].payload["userInfoList"].as_str().unwrap()).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["musicU"], "history-token-1");
    assert!(
        calls[4]
            .cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=fresh-target")
    );
    let selected = world.registry.current(Default::default()).unwrap().unwrap();
    assert_ne!(selected.selection.session_id, world.ids[0]);
    let new_credentials = selected.credentials.login_credentials().unwrap();
    assert_eq!(new_credentials.token.as_deref(), Some("fresh-target"));
    assert_eq!(
        new_credentials.refresh_token.as_deref(),
        Some("fresh-target-rt")
    );
    assert_eq!(world.session.token().unwrap().uid.as_deref(), Some("1"));
    assert_eq!(
        old_credentials.login_credentials().unwrap().revision,
        old_revision
    );
    assert_eq!(
        old_credentials
            .login_credentials()
            .unwrap()
            .token
            .as_deref(),
        Some("history-token-1")
    );
    assert!(
        !serde_json::to_string(&attempt.snapshot().unwrap())
            .unwrap()
            .contains("fresh-target")
    );
}

#[tokio::test]
async fn quick_ack_without_cookie_never_queries_or_adopts_identity() {
    let world = World::new(
        1,
        [
            json!({"code":200}).into(),
            json!({"code":200,"profile":{"userId":1}}).into(),
        ],
    )
    .await;
    let attempt = world.guest_attempt(0).await;
    let outcome = attempt.execute().await.unwrap();
    assert_eq!(outcome.phase, ServerQuickLoginPhase::ResultUnknown);
    assert!(!outcome.can_reconcile);
    assert_eq!(attempt.reconcile().await.unwrap(), outcome);
    assert_eq!(attempt.execute().await.unwrap(), outcome);
    assert!(world.session.token().unwrap().uid.is_none());
    assert_eq!(world.backend.calls().len(), 3);
}

#[tokio::test]
async fn interrupted_body_with_scoped_cookie_recovers_through_identity_only() {
    let mut response = issued_quick_reply();
    response.body_status = BodyStatus::Interrupted;
    let world = World::new(
        1,
        [
            json!({"code":200}).into(),
            HttpResponse::Ready(Ok(response)),
            json!({"code":301}).into(),
            json!({"code":200,"profile":{"userId":1}}).into(),
        ],
    )
    .await;
    let attempt = world.guest_attempt(0).await;
    let pending = attempt.execute().await.unwrap();
    assert_eq!(pending.phase, ServerQuickLoginPhase::IdentityRequired);
    assert!(pending.can_reconcile);
    assert_eq!(attempt.execute().await.unwrap(), pending);
    assert_eq!(attempt.reconcile().await.unwrap(), pending);
    assert_eq!(
        attempt.reconcile().await.unwrap().phase,
        ServerQuickLoginPhase::Authenticated
    );
    assert_eq!(
        world
            .backend
            .calls()
            .iter()
            .filter(|call| call.path == "/api/login/quicklogin")
            .count(),
        1
    );
}

#[tokio::test]
async fn quick_cancellation_drops_held_exchange_and_keeps_original_selection() {
    let (pending, release) = gate();
    let world = World::new(2, [json!({"code":200}).into(), pending]).await;
    let selection = world.registry.selection().unwrap();
    let attempt = Arc::new(world.guest_attempt(0).await);
    let job = {
        let attempt = attempt.clone();
        tokio::spawn(async move { attempt.execute().await })
    };
    world.backend.wait(4).await;
    attempt.cancel().unwrap();
    assert!(matches!(job.await.unwrap(), Err(Error::Cancelled)));
    assert!(release.send(Ok(issued_quick_reply())).is_err());
    assert_eq!(
        attempt.snapshot().unwrap().phase,
        ServerQuickLoginPhase::Cancelled
    );
    assert_eq!(world.registry.selection().unwrap(), selection);
    assert!(world.session.token().unwrap().uid.is_none());
}

#[tokio::test]
async fn removed_dormant_target_is_not_resurrected_by_late_quick_identity() {
    let (pending, release) = gate();
    let world = World::new(
        2,
        [
            json!({"code":200}).into(),
            HttpResponse::Ready(Ok(issued_quick_reply())),
            pending,
        ],
    )
    .await;
    let attempt = Arc::new(world.guest_attempt(0).await);
    let job = {
        let attempt = attempt.clone();
        tokio::spawn(async move { attempt.execute().await })
    };
    world.backend.wait(5).await;
    world
        .registry
        .remove_saved(
            &world.session,
            &world.session.token().unwrap(),
            &world.ids[0],
        )
        .unwrap();
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"profile":{"userId":1}}))))
            .is_ok()
    );
    assert!(job.await.unwrap().is_err());
    assert!(world.session.token().unwrap().uid.is_none());
    assert_eq!(world.registry.saved().unwrap().len(), 1);
    assert_eq!(
        world.registry.selection().unwrap().unwrap().session_id,
        world.ids[1]
    );
}

#[tokio::test]
async fn refresh_only_cookie_cannot_authorize_account_get() {
    let mut response = issued_quick_reply();
    response.set_cookies = vec!["MUSIC_U=refresh-only; Path=/eapi/login; Secure".into()];
    let world = World::new(
        1,
        [
            json!({"code":200}).into(),
            HttpResponse::Ready(Ok(response)),
        ],
    )
    .await;
    let attempt = world.guest_attempt(0).await;
    let view = attempt.execute().await.unwrap();
    assert_eq!(view.phase, ServerQuickLoginPhase::ResultUnknown);
    assert!(!view.can_reconcile);
    assert_eq!(attempt.reconcile().await.unwrap(), view);
    assert_eq!(world.backend.calls().len(), 3);
    assert!(world.session.token().unwrap().uid.is_none());
}

#[tokio::test]
async fn known_string_rejection_or_challenge_never_reconciles_poison_cookie() {
    for (code, expected) in [
        ("401", ServerQuickLoginPhase::Rejected),
        ("8830", ServerQuickLoginPhase::VerificationRequired),
    ] {
        let mut response = issued_quick_reply();
        response.body = serde_json::to_vec(&json!({"code":code,"private":"not exported"})).unwrap();
        let world = World::new(
            1,
            [
                json!({"code":200}).into(),
                HttpResponse::Ready(Ok(response)),
            ],
        )
        .await;
        let attempt = world.guest_attempt(0).await;
        let view = attempt.execute().await.unwrap();
        assert_eq!(view.phase, expected);
        assert!(!view.can_reconcile);
        assert_eq!(attempt.reconcile().await.unwrap(), view);
        assert_eq!(world.backend.calls().len(), 3);
        assert!(world.session.token().unwrap().uid.is_none());
    }
}

#[tokio::test]
async fn issued_cookie_cannot_adopt_a_different_identity() {
    let world = World::new(
        2,
        [
            json!({"code":200}).into(),
            HttpResponse::Ready(Ok(issued_quick_reply())),
            json!({"code":200,"profile":{"userId":2}}).into(),
        ],
    )
    .await;
    let selection = world.registry.selection().unwrap();
    let attempt = world.guest_attempt(0).await;
    let view = attempt.execute().await.unwrap();
    assert_eq!(view.phase, ServerQuickLoginPhase::Rejected);
    assert!(!view.can_reconcile);
    assert_eq!(world.registry.selection().unwrap(), selection);
    assert!(world.session.token().unwrap().uid.is_none());
}

#[tokio::test]
async fn client_requires_guest_consent_and_keeps_originating_epoch_on_success() {
    let world = World::new(
        1,
        [
            json!({"code":200}).into(),
            HttpResponse::Ready(Ok(issued_quick_reply())),
            json!({"code":200,"profile":{"userId":1}}).into(),
        ],
    )
    .await;
    assert!(
        world
            .client
            .begin_server_quick_login(&world.epoch(), &world.ids[0], true)
            .await
            .is_err()
    );
    world.session.attach(world.guest.clone()).unwrap();
    let epoch = world.epoch();
    assert!(
        world
            .client
            .begin_server_quick_login(&epoch, &world.ids[0], false)
            .await
            .is_err()
    );
    assert_eq!(world.backend.calls().len(), 1);
    let started = world
        .client
        .begin_server_quick_login(&epoch, &world.ids[0], true)
        .await
        .unwrap();
    assert_eq!(started.epoch, epoch);
    assert_eq!(started.value.state.phase, ServerQuickLoginPhase::Ready);
    assert_eq!(world.backend.calls().len(), 2);
    let id = &started.value.operation_id;
    let accepted = world
        .client
        .server_quick_login_command(&epoch, id, ServerQuickLoginCommand::Execute)
        .await
        .unwrap();
    assert_eq!(accepted.epoch, epoch);
    assert_eq!(accepted.value.operation_id, *id);
    assert_eq!(
        accepted.value.state.phase,
        ServerQuickLoginPhase::Authenticated
    );
    let wire = serde_json::to_value(&accepted.value).unwrap();
    assert_eq!(
        wire,
        json!({"operationId":id,"phase":"authenticated","canReconcile":false,"status":null})
    );
    assert_ne!(world.epoch(), epoch);
}

#[tokio::test]
async fn client_quick_cancel_releases_transition_gate_for_regular_login() {
    let (pending, release) = gate();
    let world = World::new(1, [json!({"code":200}).into(), pending]).await;
    world.session.attach(world.guest.clone()).unwrap();
    let epoch = world.epoch();
    let id = world
        .client
        .begin_server_quick_login(&epoch, &world.ids[0], true)
        .await
        .unwrap()
        .value
        .operation_id;
    let work = {
        let (client, epoch, id) = (world.client.clone(), epoch.clone(), id.clone());
        tokio::spawn(async move {
            client
                .server_quick_login_command(&epoch, &id, ServerQuickLoginCommand::Execute)
                .await
        })
    };
    world.backend.wait(3).await;
    let cancelled = world
        .client
        .server_quick_login_command(&epoch, &id, ServerQuickLoginCommand::Cancel)
        .await
        .unwrap();
    assert_eq!(
        cancelled.value.state.phase,
        ServerQuickLoginPhase::Cancelled
    );
    let next = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        world.client.begin_login(&epoch, LoginMode::Email, None),
    )
    .await
    .unwrap()
    .unwrap();
    for command in [
        ServerQuickLoginCommand::Cancel,
        ServerQuickLoginCommand::Reconcile,
    ] {
        let observed = world
            .client
            .server_quick_login_command(&epoch, &id, command)
            .await
            .unwrap();
        assert_eq!(observed.value.state.phase, ServerQuickLoginPhase::Cancelled);
    }
    assert!(world.client.login_snapshot(&next.operation_id).is_ok());
    assert!(matches!(work.await.unwrap(), Err(Error::Cancelled)));
    assert!(release.send(Ok(issued_quick_reply())).is_err());
}

#[tokio::test]
async fn regular_login_supersedes_inflight_quick_preflight_without_an_exchange() {
    let (pending, release) = gate();
    let world = World::new(1, [pending]).await;
    world.session.attach(world.guest.clone()).unwrap();
    let epoch = world.epoch();
    let preflight = {
        let (client, epoch, id) = (world.client.clone(), epoch.clone(), world.ids[0].clone());
        tokio::spawn(async move { client.begin_server_quick_login(&epoch, &id, true).await })
    };
    world.backend.wait(2).await;
    let normal = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        world.client.begin_login(&epoch, LoginMode::Email, None),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    assert!(matches!(
        preflight.await.unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(world.client.login_snapshot(&normal.operation_id).is_ok());
    assert_eq!(world.backend.calls().len(), 2);
}
