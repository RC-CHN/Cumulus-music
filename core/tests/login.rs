mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession, SessionStatus,
        login::{
            CredentialLogin, LoginGuardian, LoginStatus, PhoneCredential, PhoneLogin,
            ResendCooldown, VerificationAction, classify,
        },
        profiles::ProfileRegistry,
    },
    protocol::{
        api::ApiFuture,
        http::{ApiOrigin, ClientProfile, RequestOptions},
        wire::eapi::md5_hex,
    },
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use support::{
    ScriptApi,
    http::{HttpResponse, ScriptHttp, gate, reply},
};

struct World {
    directory: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
}
impl World {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
        let api = Arc::new(ScriptApi::new([
            json!({"code":200,"profile":{"userId":1}}).into()
        ]));
        let session = Arc::new(AccountSession::new(api, Some(registry.store())));
        session.refresh().await.unwrap();
        Self {
            directory,
            registry,
            session,
        }
    }
    fn flow(
        &self,
        http: Arc<ScriptHttp>,
        cooldown: Arc<ResendCooldown>,
        guardian: Option<Arc<dyn LoginGuardian>>,
    ) -> Arc<CredentialLogin> {
        let candidate = self
            .registry
            .candidate_with_backend(
                ClientProfile::default(),
                RequestOptions::default(),
                None,
                http,
            )
            .unwrap();
        Arc::new(CredentialLogin::new(self.session.clone(), candidate, cooldown, guardian).unwrap())
    }
    fn uid(&self) -> Option<String> {
        self.session.token().unwrap().uid
    }
}
fn cooldown() -> Arc<ResendCooldown> {
    Arc::new(ResendCooldown::default())
}

#[tokio::test]
async fn phone_payloads_match_direct_execution_of_official_module_363() {
    let world = World::new().await;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/login.json")).unwrap();
    let rows = fixture["phone"].as_array().unwrap();
    let http = Arc::new(ScriptHttp::new(
        rows.iter().map(|_| json!({"code":8820}).into()),
    ));
    let flow = world.flow(http.clone(), cooldown(), None);
    for row in rows {
        let input = &row["input"];
        let credential = if input["isPasswordLogin"] == true {
            PhoneCredential::Password(input["password"].as_str().unwrap())
        } else {
            PhoneCredential::Sms(input["captcha"].as_str().unwrap())
        };
        flow.phone(PhoneLogin {
            phone: input["phone"].as_str().unwrap(),
            country_code: input["countryCode"].as_str().unwrap(),
            credential,
            remember: input["remember"].as_bool().unwrap(),
            secure_captcha: input["secureCaptcha"].as_str(),
        })
        .await
        .unwrap();
    }
    for (call, row) in http.calls().iter().zip(rows) {
        let mut actual = call.payload.clone();
        actual.as_object_mut().unwrap().remove("header");
        let mut expected =
            cumulus_core::protocol::wire::serialization::official_data(&row["expected"]["payload"])
                .unwrap();
        expected["e_r"] = json!(true);
        assert_eq!(actual, expected);
    }
}
struct Guardian(AtomicU64);
impl LoginGuardian for Guardian {
    fn token(&self) -> ApiFuture<'_, String> {
        Box::pin(async move {
            Ok(format!(
                "synthetic-provider-{}",
                self.0.fetch_add(1, Ordering::SeqCst)
            ))
        })
    }
}

#[tokio::test]
async fn phone_captcha_verifies_identity_and_atomically_selects_restartable_profile() {
    let world = World::new().await;
    let mut authorized = reply(json!({"code":200}));
    authorized.set_cookies = vec!["MUSIC_U=synthetic-new; Path=/; Secure; HttpOnly".into()];
    authorized.refresh_token = Some("synthetic-refresh".into());
    let http = Arc::new(ScriptHttp::new([
        json!({"code":8821,"challenge":"synthetic-private"}).into(),
        HttpResponse::Ready(Ok(authorized)),
        json!({"code":200,"profile":{"userId":2}}).into(),
    ]));
    let flow = world.flow(
        http.clone(),
        cooldown(),
        Some(Arc::new(Guardian(AtomicU64::new(0)))),
    );
    flow.phone(PhoneLogin {
        phone: "13800000000",
        country_code: "86",
        credential: PhoneCredential::Password("synthetic-password"),
        remember: true,
        secure_captcha: None,
    })
    .await
    .unwrap();
    assert_eq!(flow.snapshot().unwrap().status, LoginStatus::Captcha);
    assert_eq!(world.uid().as_deref(), Some("1"));
    assert!(world.registry.selection().unwrap().is_none());
    flow.submit_captcha("user-completed-result").await.unwrap();
    assert_eq!(flow.snapshot().unwrap().status, LoginStatus::Authenticated);
    assert_eq!(world.uid().as_deref(), Some("2"));
    assert!(flow.challenge_native().unwrap().is_none());
    let calls = http.calls();
    assert_eq!(calls[0].path, "/api/w/login/cellphone");
    assert_eq!(calls[0].payload["password"], md5_hex(b"synthetic-password"));
    assert_eq!(calls[0].payload["checkToken"], "synthetic-provider-0");
    assert_eq!(calls[1].payload["checkToken"], "synthetic-provider-1");
    assert_eq!(calls[1].payload["secureCaptcha"], "user-completed-result");
    assert!(
        calls[2]
            .cookie
            .as_ref()
            .unwrap()
            .contains("MUSIC_U=synthetic-new")
    );
    assert!(!format!("{:?}", flow.snapshot().unwrap()).contains("synthetic"));
    let reopened = ProfileRegistry::open(world.directory.path(), ApiOrigin::Pc).unwrap();
    let selected = reopened.current(ClientProfile::default()).unwrap().unwrap();
    assert_eq!(
        selected.selection,
        world.registry.selection().unwrap().unwrap()
    );
    let credentials = selected.credentials.login_credentials().unwrap();
    assert_eq!(credentials.token.as_deref(), Some("synthetic-new"));
    assert_eq!(
        credentials.refresh_token.as_deref(),
        Some("synthetic-refresh")
    );
    assert_eq!(
        reopened.store().get("2", "account", "profile").unwrap(),
        Some(json!({"userId":2}))
    );
}

#[tokio::test]
async fn lost_sms_response_reserves_cooldown_across_new_attempts_and_never_retries() {
    let world = World::new().await;
    let clock = Arc::new(AtomicU64::new(0));
    let epoch = std::time::Instant::now();
    let cooldown = Arc::new(ResendCooldown::with_clock({
        let clock = clock.clone();
        Arc::new(move || epoch + std::time::Duration::from_secs(clock.load(Ordering::SeqCst)))
    }));
    let first = Arc::new(ScriptHttp::new([HttpResponse::Ready(Err(
        Error::Transport("synthetic lost body".into()),
    ))]));
    let flow = world.flow(first.clone(), cooldown.clone(), None);
    assert!(flow.send_sms("13800000000", "86", false).await.is_err());
    assert!(flow.send_sms("13800000000", "86", false).await.is_err());
    assert_eq!(first.calls().len(), 1);
    flow.cancel().unwrap();
    let second = Arc::new(ScriptHttp::new([json!({"code":200}).into()]));
    let flow = world.flow(second.clone(), cooldown, None);
    assert!(flow.send_sms("13800000000", "86", true).await.is_err());
    assert!(second.calls().is_empty());
    clock.store(61, Ordering::SeqCst);
    flow.send_sms("13800000000", "86", true).await.unwrap();
    assert_eq!(second.calls()[0].path, "/api/middle/captcha/sent/v1");
    assert_eq!(flow.snapshot().unwrap().status, LoginStatus::CodeSent);
    assert_eq!(world.uid().as_deref(), Some("1"));
}

#[tokio::test]
async fn cancelled_login_cannot_read_identity_or_replace_the_active_account() {
    let world = World::new().await;
    let (pending, release) = gate();
    let http = Arc::new(ScriptHttp::new([pending]));
    let flow = world.flow(http.clone(), cooldown(), None);
    let work = {
        let flow = flow.clone();
        tokio::spawn(async move { flow.email("me@example.test", "synthetic", true, None).await })
    };
    http.wait(1).await;
    flow.cancel().unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(http.calls().len(), 1);
    assert_eq!(world.uid().as_deref(), Some("1"));
    assert!(world.registry.selection().unwrap().is_none());
    assert_eq!(flow.snapshot().unwrap().status, LoginStatus::Cancelled);
}

#[tokio::test]
async fn dropped_future_releases_operation_and_reconciliation_does_not_replay_password() {
    let world = World::new().await;
    let (pending, _release) = gate();
    let http = Arc::new(ScriptHttp::new([
        pending,
        json!({"code":200,"profile":{"userId":2}}).into(),
    ]));
    let flow = world.flow(http.clone(), cooldown(), None);
    let work = {
        let flow = flow.clone();
        tokio::spawn(async move { flow.email("me@example.test", "synthetic", true, None).await })
    };
    http.wait(1).await;
    assert!(flow.reconcile().await.is_err());
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    let snapshot = flow.snapshot().unwrap();
    assert!(!snapshot.busy);
    assert_eq!(snapshot.error_kind, Some("interrupted"));
    flow.reconcile().await.unwrap();
    assert_eq!(world.uid().as_deref(), Some("2"));
    assert_eq!(
        http.calls()
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/api/w/login", "/api/w/nuser/account/get"]
    );
}

#[tokio::test]
async fn explicit_two_factor_answers_allow_one_replay_each_and_new_challenges_remain_pending() {
    let world = World::new().await;
    let http = Arc::new(ScriptHttp::new([
        json!({"code":8830,"MUSIC_TU":"synthetic-ticket"}).into(),
        json!({"code":200,"data":{"types":["EMAIL"]}}).into(),
        json!({"code":200}).into(),
        json!({"code":400}).into(),
        json!({"code":200}).into(),
        json!({"code":301}).into(),
        json!({"code":8861}).into(),
        json!({"code":200}).into(),
        json!({"code":301}).into(),
        json!({"code":200}).into(),
        json!({"code":200,"profile":{"userId":2}}).into(),
    ]));
    let flow = world.flow(http.clone(), cooldown(), None);
    flow.email("me@example.test", "synthetic", true, None)
        .await
        .unwrap();
    flow.verification_methods().await.unwrap();
    assert_eq!(http.calls()[1].payload["bindingCellphoneEnable"], "true");
    assert!(
        http.calls()[1]
            .cookie
            .as_ref()
            .unwrap()
            .contains("MUSIC_TU=synthetic-ticket")
    );
    flow.verification(VerificationAction::MailVerify, json!({"captcha":"bad"}))
        .await
        .unwrap();
    assert_eq!(flow.snapshot().unwrap().status, LoginStatus::TwoFactor);
    flow.verification(
        VerificationAction::MailVerify,
        json!({"captcha":"user-code"}),
    )
    .await
    .unwrap();
    assert_eq!(
        flow.snapshot().unwrap().status,
        LoginStatus::SecurityQuestion
    );
    assert_eq!(world.uid().as_deref(), Some("1"));
    flow.verification(
        VerificationAction::QuestionVerify,
        json!({"ans":"user-answer"}),
    )
    .await
    .unwrap();
    assert_eq!(world.uid().as_deref(), Some("2"));
    assert_eq!(
        http.calls()
            .iter()
            .filter(|c| c.path == "/api/w/login")
            .count(),
        3
    );
    assert!(
        !http
            .calls()
            .iter()
            .any(|c| c.path.ends_with("/send") || c.path.ends_with("/sent"))
    );
}

#[tokio::test]
async fn late_verification_does_not_begin_a_second_action_after_cancellation() {
    let world = World::new().await;
    let (pending, release) = gate();
    let http = Arc::new(ScriptHttp::new([json!({"code":8830}).into(), pending]));
    let flow = world.flow(http.clone(), cooldown(), None);
    flow.email("me@example.test", "synthetic", true, None)
        .await
        .unwrap();
    let work = {
        let flow = flow.clone();
        tokio::spawn(async move { flow.verification_methods().await })
    };
    http.wait(2).await;
    flow.cancel().unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(http.calls().len(), 2);
    assert_eq!(world.uid().as_deref(), Some("1"));
}

#[tokio::test]
async fn account_switch_during_identity_read_prevents_candidate_adoption() {
    let world = World::new().await;
    let (pending, release) = gate();
    let http = Arc::new(ScriptHttp::new([pending]));
    let flow = world.flow(http.clone(), cooldown(), None);
    let work = {
        let flow = flow.clone();
        tokio::spawn(async move { flow.reconcile().await })
    };
    http.wait(1).await;
    world.session.detach().unwrap();
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"profile":{"userId":2}}))))
            .is_ok()
    );
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        world.session.snapshot().unwrap().status,
        SessionStatus::SignedOut
    );
    assert!(world.registry.selection().unwrap().is_none());
}

#[tokio::test]
async fn concurrent_candidates_compare_selection_and_rollback_unselected_profile_cache() {
    let world = World::new().await;
    let http1 = Arc::new(ScriptHttp::new([
        json!({"code":200,"profile":{"userId":2}}).into(),
    ]));
    let http2 = Arc::new(ScriptHttp::new([
        json!({"code":200,"profile":{"userId":3}}).into(),
    ]));
    let first = world
        .registry
        .candidate_with_backend(
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http1,
        )
        .unwrap();
    let second = world
        .registry
        .candidate_with_backend(
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http2,
        )
        .unwrap();
    assert_ne!(
        first.credentials().device_id().unwrap(),
        second.credentials().device_id().unwrap()
    );
    let other = AccountSession::new(Arc::new(ScriptApi::new([])), Some(world.registry.store()));
    first
        .verify_and_adopt(&world.session, &world.session.token().unwrap())
        .await
        .unwrap();
    let selection = world.registry.selection().unwrap();
    assert!(matches!(
        second
            .verify_and_adopt(&other, &other.token().unwrap())
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(world.registry.selection().unwrap(), selection);
    assert!(
        world
            .registry
            .store()
            .get("3", "account", "profile")
            .unwrap()
            .is_none()
    );
    assert_eq!(world.uid().as_deref(), Some("2"));
}

#[tokio::test]
async fn failed_selection_commit_preserves_current_identity_and_can_reconcile_without_relogin() {
    let world = World::new().await;
    let http = Arc::new(ScriptHttp::new([
        json!({"code":200}).into(),
        json!({"code":200,"profile":{"userId":2}}).into(),
        json!({"code":200,"profile":{"userId":2}}).into(),
    ]));
    let flow = world.flow(http.clone(), cooldown(), None);
    let db = rusqlite::Connection::open(world.directory.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_selection BEFORE INSERT ON state WHEN NEW.kind='profiles' AND NEW.key='active' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").unwrap();
    assert!(matches!(
        flow.email("me@example.test", "synthetic", true, None).await,
        Err(Error::Storage(_))
    ));
    assert!(world.registry.selection().unwrap().is_none());
    assert!(
        world
            .registry
            .store()
            .get("2", "account", "profile")
            .unwrap()
            .is_none()
    );
    assert_eq!(world.uid().as_deref(), Some("1"));
    db.execute_batch("DROP TRIGGER reject_selection").unwrap();
    flow.reconcile().await.unwrap();
    assert_eq!(world.uid().as_deref(), Some("2"));
    assert_eq!(
        http.calls()
            .iter()
            .filter(|c| c.path == "/api/w/login")
            .count(),
        1
    );
}

#[tokio::test]
async fn invalid_inputs_and_non_applicable_verification_stop_before_network() {
    let world = World::new().await;
    let http = Arc::new(ScriptHttp::new([]));
    let flow = world.flow(http.clone(), cooldown(), None);
    assert!(
        flow.email("bad-address", "synthetic", true, None)
            .await
            .is_err()
    );
    assert!(flow.email("me@example.test", "", true, None).await.is_err());
    assert!(
        flow.phone(PhoneLogin {
            phone: "13800000000",
            country_code: "86",
            credential: PhoneCredential::Sms("abcdef"),
            remember: true,
            secure_captcha: None
        })
        .await
        .is_err()
    );
    assert!(flow.submit_captcha("user-result").await.is_err());
    assert!(flow.verification_methods().await.is_err());
    assert!(
        flow.verification(VerificationAction::MailVerify, json!({"password":"extra"}))
            .await
            .is_err()
    );
    assert!(http.calls().is_empty());
    for (code, status) in [
        (200, LoginStatus::Authorized),
        (702, LoginStatus::Verification),
        (8840, LoginStatus::Verification),
        (8820, LoginStatus::UpgradeRequired),
        (8821, LoginStatus::Captcha),
        (8830, LoginStatus::TwoFactor),
        (8860, LoginStatus::PhoneReuse),
        (8861, LoginStatus::SecurityQuestion),
        (509, LoginStatus::RateLimited),
        (304, LoginStatus::AccountDisabled),
        (999, LoginStatus::Rejected),
    ] {
        assert_eq!(classify(&json!({"code":code})), status);
    }
    for code in [8810, 10004] {
        for (kind, status) in [
            (1, LoginStatus::SecurityQr),
            (2, LoginStatus::UpgradeRequired),
            (3, LoginStatus::RiskNotice),
            (4, LoginStatus::Rejected),
        ] {
            assert_eq!(
                classify(&json!({"code":code,"toast":{"type":kind}})),
                status
            );
        }
    }
    assert_eq!(classify(&json!({"code":"200"})), LoginStatus::Rejected);
}

#[tokio::test]
async fn profile_registry_rejects_other_origins_and_malformed_or_redirected_selection() {
    let world = World::new().await;
    assert!(ProfileRegistry::open(world.directory.path(), ApiOrigin::Interface).is_err());
    world
        .registry
        .store()
        .put(
            "transport",
            "profiles",
            "active",
            &json!({"session_id":"../escape","revision":"a".repeat(32)}),
        )
        .unwrap();
    assert!(world.registry.current(ClientProfile::default()).is_err());
    world
        .registry
        .store()
        .delete("transport", "profiles", "active")
        .unwrap();
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), world.directory.path().join("sessions"))
            .unwrap();
        assert!(world.registry.candidate(ClientProfile::default()).is_err());
    }
}
