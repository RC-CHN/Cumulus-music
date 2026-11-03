mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession, QrStatus,
        login::{CredentialLogin, LoginProtocol, LoginStatus, ResendCooldown, SecurityQrView},
        profiles::ProfileRegistry,
    },
    protocol::http::{ApiOrigin, BodyStatus, ClientProfile, RequestOptions},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{
    ScriptApi,
    http::{HttpResponse, ScriptHttp, gate, reply},
};

struct World {
    _directory: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    http: Arc<ScriptHttp>,
    flow: Arc<CredentialLogin>,
}
impl World {
    async fn new(
        protocol: LoginProtocol,
        challenge: Value,
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(
            Arc::new(ScriptApi::new([
                json!({"code":200,"profile":{"userId":1}}).into()
            ])),
            Some(registry.store()),
        ));
        session.refresh().await.unwrap();
        let http = Arc::new(ScriptHttp::new(
            std::iter::once(challenge.into()).chain(replies),
        ));
        let candidate = registry
            .candidate_with_backend(
                ClientProfile::default(),
                RequestOptions::default(),
                None,
                http.clone(),
            )
            .unwrap();
        let flow = Arc::new(
            CredentialLogin::new_with_protocol(
                session.clone(),
                candidate,
                Arc::new(ResendCooldown::default()),
                None,
                protocol,
            )
            .unwrap(),
        );
        flow.email("qr@example.test", "synthetic-password", true, None)
            .await
            .unwrap();
        assert_eq!(flow.snapshot().unwrap().status, LoginStatus::SecurityQr);
        Self {
            _directory: directory,
            registry,
            session,
            http,
            flow,
        }
    }
    fn qr(&self) -> SecurityQrView {
        self.flow.security_qr_snapshot().unwrap().unwrap()
    }
    fn uid(&self) -> Option<String> {
        self.session.token().unwrap().uid
    }
}

fn challenge(code: i64, key: Option<&str>) -> Value {
    json!({"code":code,"toast":{"type":1,"unikey":key,"nickname":"Synthetic account"}})
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2,"nickname":"Synthetic account"}}).into()
}

fn qr_ecc_format_bits(modules: &[Vec<bool>]) -> u8 {
    // QR format bits 14/13 sit at (0,8)/(1,8), before XOR mask 0x5412.
    // ECC L encodes 01; this reads the public matrix independently of its encoder.
    ((u8::from(modules[8][0]) << 1) | u8::from(modules[8][1])) ^ 0b10
}

#[tokio::test]
async fn seeded_tickets_render_immediately_and_use_legacy_type_three_for_both_login_protocols() {
    const SEEDED_KEY: &str = "synthetic-first-security-ticket-000001";
    for (protocol, code) in [(LoginProtocol::Legacy, 8810), (LoginProtocol::Jsk, 10004)] {
        let world = World::new(
            protocol,
            challenge(code, Some(SEEDED_KEY)),
            [
                json!({"code":801}).into(),
                json!({"code":802}).into(),
                json!({"code":800}).into(),
                json!({"code":200,"unikey":"synthetic-refreshed-ticket"}).into(),
            ],
        )
        .await;
        let first = world.qr();
        assert_eq!(first.status, QrStatus::WaitingScan);
        assert_eq!(first.poll_after_ms, 1000);
        assert!(first.can_refresh);
        assert!(!first.can_reconcile);
        assert_eq!(first.nickname.as_deref(), Some("Synthetic account"));
        assert_eq!(world.http.calls().len(), 1);
        // This 74-byte native URL requires version 4 at recovered ECC L
        // (33 modules); ECC M would require version 5 (37 modules).
        let rendered = first.modules.as_ref().unwrap();
        assert_eq!(rendered.len(), 33);
        assert!(rendered.iter().all(|row| row.len() == 33));
        assert_eq!(qr_ecc_format_bits(rendered), 0b01);
        let public = serde_json::to_string(&first).unwrap();
        for secret in [SEEDED_KEY, "codekey", "https://", "synthetic-password"] {
            assert!(!public.contains(secret));
        }
        let serialized = serde_json::to_value(&first).unwrap();
        assert_eq!(serialized["pollAfterMs"], 1000);
        assert_eq!(serialized["canRefresh"], true);
        assert_eq!(serialized["canReconcile"], false);
        for status in [
            QrStatus::WaitingScan,
            QrStatus::WaitingConfirmation,
            QrStatus::Expired,
        ] {
            assert_eq!(
                world
                    .flow
                    .security_qr_poll(&first.id, None)
                    .await
                    .unwrap()
                    .status,
                status
            );
        }
        assert!(world.qr().modules.is_none());
        let refreshed = world.flow.security_qr_refresh(&first.id).await.unwrap();
        assert_eq!(refreshed.status, QrStatus::WaitingScan);
        assert_ne!(refreshed.modules, first.modules);
        let calls = world.http.calls();
        for call in &calls[1..4] {
            assert_eq!(call.path, "/api/login/qrcode/client/login");
            assert_eq!(call.payload["key"], SEEDED_KEY);
            assert_eq!(call.payload["type"], "3");
            assert!(call.payload.get("fromPage").is_none());
            assert!(call.payload.get("secureCaptcha").is_none());
        }
        assert_eq!(calls[4].path, "/api/login/qrcode/unikey");
        assert_eq!(calls[4].payload["type"], "3");
        assert_eq!(calls[4].payload["lastUniKey"], SEEDED_KEY);
        assert!(calls[4].payload.get("fromPage").is_none());
        assert_eq!(world.uid().as_deref(), Some("1"));
        assert!(world.registry.selection().unwrap().is_none());
    }
}

#[tokio::test]
async fn missing_or_empty_ticket_stays_idle_until_an_explicit_initial_refresh() {
    for key in [None, Some("")] {
        let world = World::new(
            LoginProtocol::Legacy,
            challenge(8810, key),
            [json!({"code":200,"unikey":"synthetic-created-ticket"}).into()],
        )
        .await;
        let initial = world.qr();
        assert_eq!(initial.status, QrStatus::Idle);
        assert!(initial.can_refresh);
        assert!(!initial.can_reconcile);
        assert!(initial.modules.is_none());
        assert_eq!(world.http.calls().len(), 1);
        assert!(
            world
                .flow
                .security_qr_poll(&initial.id, None)
                .await
                .is_err()
        );
        assert_eq!(world.http.calls().len(), 1);
        assert_eq!(
            world
                .flow
                .security_qr_refresh(&initial.id)
                .await
                .unwrap()
                .status,
            QrStatus::WaitingScan
        );
        let calls = world.http.calls();
        assert_eq!(calls[1].path, "/api/login/qrcode/unikey");
        assert_eq!(calls[1].payload["type"], "3");
        assert!(calls[1].payload.get("lastUniKey").is_none());
    }
}

#[tokio::test]
async fn a_failed_refresh_stops_polling_and_retains_the_old_ticket_for_explicit_refresh() {
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-old-ticket")),
        [
            HttpResponse::Ready(Err(Error::Transport("synthetic lost refresh".into()))),
            json!({"code":200,"unikey":"synthetic-new-ticket"}).into(),
        ],
    )
    .await;
    let id = world.qr().id;
    assert!(world.flow.security_qr_refresh(&id).await.is_err());
    assert_eq!(world.qr().status, QrStatus::Error);
    assert!(world.qr().can_refresh);
    assert!(!world.qr().can_reconcile);
    assert!(world.qr().modules.is_none());
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert_eq!(world.http.calls().len(), 2);
    world.flow.security_qr_refresh(&id).await.unwrap();
    for call in &world.http.calls()[1..] {
        assert_eq!(call.path, "/api/login/qrcode/unikey");
        assert_eq!(call.payload["lastUniKey"], "synthetic-old-ticket");
    }
}

#[tokio::test]
async fn nickname_projection_does_not_turn_an_untrusted_url_into_public_login_data() {
    let mut response = challenge(10004, Some("synthetic-private-ticket"));
    response["toast"]["nickname"] =
        "https://music.163.com/login?codekey=synthetic-private-ticket".into();
    let world = World::new(LoginProtocol::Jsk, response, []).await;
    let view = world.qr();
    assert!(view.nickname.is_none());
    assert_eq!(qr_ecc_format_bits(view.modules.as_ref().unwrap()), 0b01);
    let public = serde_json::to_string(&view).unwrap();
    assert!(!public.contains("codekey"));
    assert!(!public.contains("synthetic-private-ticket"));
}

#[tokio::test]
async fn captcha_completion_polls_the_same_ticket_without_replaying_password_login() {
    let world = World::new(
        LoginProtocol::Jsk,
        challenge(10004, Some("synthetic-captcha-ticket")),
        [
            json!({"code":8821,"challenge":"synthetic-private-captcha"}).into(),
            json!({"code":801}).into(),
        ],
    )
    .await;
    let id = world.qr().id;
    assert_eq!(
        world.flow.security_qr_poll(&id, None).await.unwrap().status,
        QrStatus::Challenge
    );
    assert!(world.flow.snapshot().unwrap().captcha_available);
    assert!(world.flow.challenge_native().unwrap().is_some());
    assert_eq!(
        world.flow.security_qr_poll(&id, None).await.unwrap().status,
        QrStatus::Challenge
    );
    assert_eq!(world.http.calls().len(), 2);
    assert!(world.flow.security_qr_poll(&id, Some("")).await.is_err());
    assert_eq!(
        world
            .flow
            .security_qr_poll(&id, Some("synthetic-completed-proof"))
            .await
            .unwrap()
            .status,
        QrStatus::WaitingScan
    );
    assert!(world.flow.challenge_native().unwrap().is_none());
    let calls = world.http.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2].path, "/api/login/qrcode/client/login");
    assert_eq!(calls[2].payload["key"], "synthetic-captcha-ticket");
    assert_eq!(calls[2].payload["type"], "3");
    assert_eq!(
        calls[2].payload["secureCaptcha"],
        "synthetic-completed-proof"
    );
    assert!(calls[2].payload.get("password").is_none());
    assert!(calls[2].payload.get("fromPage").is_none());
}

#[tokio::test]
async fn authorization_and_lost_identity_response_reconcile_only_the_same_candidate() {
    let mut authorized = reply(json!({"code":803}));
    authorized.set_cookies = vec!["MUSIC_U=synthetic-qr-user; Path=/; Secure; HttpOnly".into()];
    authorized.refresh_token = Some("synthetic-qr-refresh".into());
    let world = World::new(
        LoginProtocol::Jsk,
        challenge(8810, Some("synthetic-authorized-ticket")),
        [
            HttpResponse::Ready(Ok(authorized)),
            HttpResponse::Ready(Err(Error::Transport(
                "synthetic lost identity response".into(),
            ))),
            HttpResponse::Ready(Err(Error::Transport(
                "synthetic lost reconciliation".into(),
            ))),
            identity(),
        ],
    )
    .await;
    let id = world.qr().id;
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert_eq!(world.qr().status, QrStatus::Error);
    assert!(!world.qr().can_refresh);
    assert!(world.qr().can_reconcile);
    assert_eq!(world.uid().as_deref(), Some("1"));
    assert!(world.registry.selection().unwrap().is_none());
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert!(world.flow.security_qr_refresh(&id).await.is_err());
    assert_eq!(world.http.calls().len(), 3);
    assert!(matches!(
        world.flow.security_qr_reconcile(&id).await,
        Err(Error::Transport(_))
    ));
    assert!(!world.qr().can_refresh);
    assert!(world.qr().can_reconcile);
    assert_eq!(world.uid().as_deref(), Some("1"));
    assert_eq!(
        world.flow.security_qr_reconcile(&id).await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(world.uid().as_deref(), Some("2"));
    let calls = world.http.calls();
    assert_eq!(calls.len(), 5);
    assert_eq!(calls[1].path, "/api/login/qrcode/client/login");
    for call in &calls[2..] {
        assert_eq!(call.path, "/api/w/nuser/account/get");
        assert!(
            call.cookie
                .as_ref()
                .unwrap()
                .contains("MUSIC_U=synthetic-qr-user")
        );
    }
    let selected = world
        .registry
        .current(ClientProfile::default())
        .unwrap()
        .unwrap();
    let saved = selected.credentials.login_credentials().unwrap();
    assert_eq!(saved.token.as_deref(), Some("synthetic-qr-user"));
    assert_eq!(saved.refresh_token.as_deref(), Some("synthetic-qr-refresh"));
}

#[tokio::test]
async fn an_explicit_reconciliation_301_allows_a_new_user_refresh_without_replaying_login() {
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-unconfirmed-ticket")),
        [
            json!({"code":803}).into(),
            HttpResponse::Ready(Err(Error::Transport("synthetic lost identity".into()))),
            json!({"code":301}).into(),
            json!({"code":200,"unikey":"synthetic-explicit-new-ticket"}).into(),
            json!({"code":801}).into(),
        ],
    )
    .await;
    let id = world.qr().id;
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert!(!world.qr().can_refresh);
    assert!(world.qr().can_reconcile);
    assert!(
        world
            .flow
            .security_qr_reconcile(&id)
            .await
            .unwrap_err()
            .is_service_code(301)
    );
    assert_eq!(world.qr().status, QrStatus::Error);
    assert!(world.qr().can_refresh);
    assert!(!world.qr().can_reconcile);
    assert_eq!(world.http.calls().len(), 4);
    assert!(world.flow.security_qr_reconcile(&id).await.is_err());
    assert_eq!(world.http.calls().len(), 4);
    world.flow.security_qr_refresh(&id).await.unwrap();
    assert_eq!(world.http.calls()[4].path, "/api/login/qrcode/unikey");
    assert_eq!(
        world.http.calls()[4].payload["lastUniKey"],
        "synthetic-unconfirmed-ticket"
    );
    world.flow.security_qr_poll(&id, None).await.unwrap();
    assert_eq!(
        world.http.calls()[5].payload["key"],
        "synthetic-explicit-new-ticket"
    );
    assert_eq!(world.uid().as_deref(), Some("1"));
    assert!(world.registry.selection().unwrap().is_none());
    assert_eq!(
        world
            .http
            .calls()
            .iter()
            .filter(|call| call.path == "/api/w/login")
            .count(),
        1
    );
}

#[tokio::test]
async fn lost_authorization_body_stops_polling_and_reads_identity_without_accepting_partial_refresh()
 {
    let mut interrupted = reply(json!({"code":803}));
    interrupted.body_status = BodyStatus::Interrupted;
    interrupted.set_cookies = vec!["MUSIC_U=synthetic-qr-user; Path=/; Secure; HttpOnly".into()];
    interrupted.refresh_token = Some("synthetic-unconfirmed-refresh".into());
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-lost-ticket")),
        [HttpResponse::Ready(Ok(interrupted)), identity()],
    )
    .await;
    let id = world.qr().id;
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert_eq!(world.qr().status, QrStatus::Error);
    assert!(!world.qr().can_refresh);
    assert!(world.qr().can_reconcile);
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert!(world.flow.security_qr_refresh(&id).await.is_err());
    assert_eq!(world.http.calls().len(), 2);
    world.flow.security_qr_reconcile(&id).await.unwrap();
    assert_eq!(world.uid().as_deref(), Some("2"));
    assert_eq!(world.http.calls()[2].path, "/api/w/nuser/account/get");
    let selected = world
        .registry
        .current(ClientProfile::default())
        .unwrap()
        .unwrap();
    assert!(
        selected
            .credentials
            .login_credentials()
            .unwrap()
            .refresh_token
            .is_none()
    );
}

#[tokio::test]
async fn cancellation_revokes_pending_creation_poll_and_identity_reads_before_they_can_adopt() {
    for stage in ["create", "poll", "identity"] {
        let (pending, release) = gate();
        let replies = if stage == "identity" {
            vec![json!({"code":803}).into(), pending]
        } else {
            vec![pending]
        };
        let world = World::new(
            LoginProtocol::Legacy,
            challenge(
                8810,
                if stage == "create" {
                    None
                } else {
                    Some("synthetic-cancel-ticket")
                },
            ),
            replies,
        )
        .await;
        let id = world.qr().id;
        let work = {
            let flow = world.flow.clone();
            tokio::spawn(async move {
                if stage == "create" {
                    flow.security_qr_refresh(&id).await
                } else {
                    flow.security_qr_poll(&id, None).await
                }
            })
        };
        let count = if stage == "identity" { 3 } else { 2 };
        world.http.wait(count).await;
        assert!(!world.qr().can_refresh);
        assert!(!world.qr().can_reconcile);
        world.flow.cancel().unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), work)
                .await
                .unwrap()
                .unwrap(),
            Err(Error::StaleResponse)
        ));
        let _ = release.send(Ok(reply(if stage == "create" {
            json!({"code":200,"unikey":"synthetic-late-ticket"})
        } else if stage == "poll" {
            json!({"code":803})
        } else {
            json!({"code":200,"profile":{"userId":2}})
        })));
        assert_eq!(world.http.calls().len(), count);
        assert_eq!(
            world.flow.snapshot().unwrap().status,
            LoginStatus::Cancelled
        );
        assert!(world.flow.security_qr_snapshot().unwrap().is_none());
        assert_eq!(world.uid().as_deref(), Some("1"));
        assert!(world.registry.selection().unwrap().is_none());
    }
}

#[tokio::test]
async fn dropped_poll_releases_busy_and_stops_automatic_polling_until_explicit_recovery() {
    let (pending, mut release) = gate();
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-drop-ticket")),
        [pending, identity()],
    )
    .await;
    let id = world.qr().id;
    let work = {
        let flow = world.flow.clone();
        let id = id.clone();
        tokio::spawn(async move { flow.security_qr_poll(&id, None).await })
    };
    world.http.wait(2).await;
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert_eq!(world.http.calls().len(), 2);
    work.abort();
    assert!(matches!(work.await, Err(error) if error.is_cancelled()));
    release.closed().await;
    assert!(!world.flow.snapshot().unwrap().busy);
    assert_eq!(
        world.flow.snapshot().unwrap().error_kind,
        Some("interrupted")
    );
    assert_eq!(world.qr().status, QrStatus::Error);
    assert!(!world.qr().can_refresh);
    assert!(world.qr().can_reconcile);
    assert!(world.flow.security_qr_poll(&id, None).await.is_err());
    assert!(world.flow.security_qr_refresh(&id).await.is_err());
    world.flow.security_qr_reconcile(&id).await.unwrap();
    assert_eq!(world.uid().as_deref(), Some("2"));
    assert_eq!(world.http.calls()[2].path, "/api/w/nuser/account/get");
}

#[tokio::test]
async fn a_changed_active_account_revokes_a_late_security_qr_identity_commit() {
    let (pending, release) = gate();
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-account-switch-ticket")),
        [json!({"code":803}).into(), pending],
    )
    .await;
    let id = world.qr().id;
    let work = {
        let flow = world.flow.clone();
        tokio::spawn(async move { flow.security_qr_poll(&id, None).await })
    };
    world.http.wait(3).await;
    world.session.detach().unwrap();
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"profile":{"userId":2}}))))
            .is_ok()
    );
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(world.uid().is_none());
    assert!(world.registry.selection().unwrap().is_none());
}

#[tokio::test]
async fn failed_selection_persistence_can_only_retry_identity_without_resubmitting_qr() {
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-persistence-ticket")),
        [json!({"code":803}).into(), identity(), identity()],
    )
    .await;
    let database =
        rusqlite::Connection::open(world._directory.path().join("core.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER reject_security_qr_selection BEFORE INSERT ON state
         WHEN NEW.kind='profiles' AND NEW.key='active'
         BEGIN SELECT RAISE(ABORT,'synthetic security QR selection failure'); END;",
        )
        .unwrap();
    let id = world.qr().id;
    assert!(matches!(
        world.flow.security_qr_poll(&id, None).await,
        Err(Error::Storage(_))
    ));
    assert!(!world.qr().can_refresh);
    assert!(world.qr().can_reconcile);
    assert_eq!(world.uid().as_deref(), Some("1"));
    assert!(world.registry.selection().unwrap().is_none());
    database
        .execute_batch("DROP TRIGGER reject_security_qr_selection")
        .unwrap();
    world.flow.security_qr_reconcile(&id).await.unwrap();
    assert_eq!(world.uid().as_deref(), Some("2"));
    let paths = world
        .http
        .calls()
        .into_iter()
        .map(|call| call.path)
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "/api/w/login",
            "/api/login/qrcode/client/login",
            "/api/w/nuser/account/get",
            "/api/w/nuser/account/get",
        ]
    );
}

#[tokio::test]
async fn stale_view_ids_and_untrusted_captcha_proofs_cannot_issue_qr_requests() {
    let world = World::new(
        LoginProtocol::Legacy,
        challenge(8810, Some("synthetic-current-ticket")),
        [],
    )
    .await;
    for id in ["", "unrelated-view"] {
        assert!(matches!(
            world.flow.security_qr_poll(id, None).await,
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            world.flow.security_qr_refresh(id).await,
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            world.flow.security_qr_reconcile(id).await,
            Err(Error::StaleResponse)
        ));
    }
    let id = world.qr().id;
    for proof in ["", "synthetic\r\nproof", "synthetic-proof-before-challenge"] {
        assert!(world.flow.security_qr_poll(&id, Some(proof)).await.is_err());
    }
    assert_eq!(world.http.calls().len(), 1);
    assert!(!world.flow.snapshot().unwrap().busy);
    assert_eq!(world.qr().status, QrStatus::WaitingScan);
}
