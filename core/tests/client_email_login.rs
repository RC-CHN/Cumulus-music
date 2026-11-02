mod support;
use cumulus_core::{
    Error,
    account::{
        SessionStatus,
        login::{EmailPhase as Phase, LoginStatus},
        profiles::ProfileRegistry,
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode},
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, BodyStatus},
    },
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use support::http::{HttpResponse, ScriptHttp, gate, reply};
struct World {
    root: tempfile::TempDir,
    http: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
    id: String,
}
fn sent() -> HttpResponse {
    json!({"code":200,"data":{"mailAddress":"m***@example.test"}}).into()
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2}}).into()
}
fn authorized(data: Value) -> HttpResponse {
    let mut r = reply(json!({"code":200,"data":data}));
    r.set_cookies = vec!["MUSIC_U=synthetic-email; Path=/; Secure; HttpOnly".into()];
    r.refresh_token = Some("synthetic-email-refresh".into());
    HttpResponse::Ready(Ok(r))
}
impl World {
    async fn new(
        jsk: bool,
        offered: bool,
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        Self::with_context(jsk, offered, replies, None).await
    }
    async fn with_context(
        jsk: bool,
        offered: bool,
        replies: impl IntoIterator<Item = HttpResponse>,
        context: Option<Arc<RequestContext>>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut all = vec![];
        if jsk {
            all.push(json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into());
        }
        all.push(json!({"code":8830}).into());
        let list = json!({"code":200,"data":{"types":if offered {json!(["SECURITY_EMAIL","UP_SMS"])}else{json!(["UP_SMS"])}}});
        if jsk {
            all.extend([json!({"code":200}).into(), list.into()]);
        } else {
            all.extend([list.into(), json!({"code":200}).into()]);
        }
        all.extend(replies);
        let http = Arc::new(ScriptHttp::new(all));
        let c = MusicClient::open_with_request_context(
            root.path(),
            None,
            Default::default(),
            Some(http.clone()),
            context,
        )
        .unwrap();
        let client = Arc::new(if jsk {
            c.with_official_login_policy()
        } else {
            c
        });
        let id = client
            .begin_login(&client.account().unwrap().epoch, LoginMode::Email, None)
            .await
            .unwrap()
            .operation_id;
        client
            .login_command(
                &id,
                Cmd::Email {
                    address: "me@example.test".into(),
                    password: "synthetic-password".into(),
                    remember: true,
                },
            )
            .await
            .unwrap();
        client
            .login_command(&id, Cmd::VerificationMethods)
            .await
            .unwrap();
        Self {
            root,
            http,
            client,
            id,
        }
    }
    async fn start(&self) -> String {
        self.client
            .login_command(&self.id, Cmd::SecurityMailSend)
            .await
            .unwrap()
            .email
            .unwrap()
            .id
    }
    async fn verify(
        &self,
        id: &str,
        code: &str,
    ) -> cumulus_core::Result<cumulus_core::client::dto::LoginView> {
        self.client
            .login_command(
                &self.id,
                Cmd::SecurityMailVerify {
                    verification_id: id.into(),
                    code: code.into(),
                },
            )
            .await
    }
    fn phase(&self) -> Phase {
        self.client
            .login_snapshot(&self.id)
            .unwrap()
            .email
            .unwrap()
            .phase
    }
    fn calls(&self, path: &str) -> usize {
        self.http.calls().iter().filter(|c| c.path == path).count()
    }
}
const SEND: &str = "/api/user/2fa/securityEmail/send";
const VERIFY: &str = "/api/user/2fa/securityEmail/verify";

#[tokio::test]
async fn both_protocols_preserve_fields_limits_new_attempt_ids_and_real_identity_adoption() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            true,
            [
                sent(),
                json!({"code":4012}).into(),
                json!({"code":4013}).into(),
                sent(),
                authorized(json!({"success":false})),
                authorized(json!({"success":true})),
                identity(),
            ],
        )
        .await;
        let id = w.start().await;
        assert!(w.phase() == Phase::CodeRequired);
        assert_eq!(
            w.client
                .login_snapshot(&w.id)
                .unwrap()
                .email
                .unwrap()
                .mail_address
                .as_deref(),
            Some("m***@example.test")
        );
        for code in ["", "12345", "1234567", "１２３４５６", "abcdef"] {
            assert!(w.verify(&id, code).await.is_err());
        }
        assert_eq!(w.calls(VERIFY), 0);
        let wrong = w.verify(&id, "111111").await.unwrap();
        assert!(wrong.email.unwrap().answer_rejected && w.phase() == Phase::CodeRequired);
        w.verify(&id, "222222").await.unwrap();
        assert!(w.phase() == Phase::VerifyLimit);
        assert!(w.verify(&id, "123456").await.is_err());
        let next = w
            .client
            .login_command(
                &w.id,
                Cmd::SecurityMailResend {
                    verification_id: id.clone(),
                },
            )
            .await
            .unwrap()
            .email
            .unwrap()
            .id;
        assert_ne!(id, next);
        assert!(w.verify(&id, "123456").await.is_err());
        let failed = w.verify(&next, "123456").await.unwrap();
        assert!(failed.email.unwrap().answer_rejected);
        assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
        assert_eq!(w.calls("/api/w/nuser/account/get"), 0);
        assert_eq!(
            w.verify(&next, "123456").await.unwrap().status,
            LoginStatus::Authenticated
        );
        let saved = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
            .unwrap()
            .current(Default::default())
            .unwrap()
            .unwrap()
            .credentials
            .login_credentials()
            .unwrap();
        assert_eq!(
            saved.refresh_token.as_deref(),
            Some("synthetic-email-refresh")
        );
        assert_eq!(saved.last_login_at.is_some(), jsk);
        assert_eq!(w.calls(SEND), 2);
        assert_eq!(w.calls(VERIFY), 4);
        for call in w
            .http
            .calls()
            .iter()
            .filter(|c| [SEND, VERIFY].contains(&c.path.as_str()))
        {
            assert_eq!(call.payload["scene"], "0");
            assert_eq!(call.payload.get("fromPage").is_some(), jsk);
            assert!(call.payload.get("mailAddress").is_none());
            if call.path == SEND || call.payload["captcha"] == "123456" {
                let frozen: Value =
                    serde_json::from_str(include_str!("fixtures/email-login-official.json"))
                        .unwrap();
                let key = format!(
                    "{}{}",
                    if jsk { "jsk" } else { "legacy" },
                    if call.path == SEND { "Send" } else { "Verify" }
                );
                let mut actual = call.payload.clone();
                actual.as_object_mut().unwrap().remove("header");
                actual.as_object_mut().unwrap().remove("e_r");
                assert_eq!(actual, frozen["vectors"][&key]["payload"]);
                assert_eq!(call.path, frozen["vectors"][&key]["path"]);
            }
        }
    }
}
#[tokio::test]
async fn offered_methods_and_send_limit_are_enforced_natively_without_repeated_email() {
    let w = World::new(true, false, []).await;
    assert!(
        w.client
            .login_command(&w.id, Cmd::SecurityMailSend)
            .await
            .is_err()
    );
    assert_eq!(w.calls(SEND), 0);
    let w = World::new(true, true, [json!({"code":4011}).into()]).await;
    let id = w.start().await;
    assert!(w.phase() == Phase::SendLimit);
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::SecurityMailResend {
                    verification_id: id.clone()
                }
            )
            .await
            .is_err()
    );
    assert!(w.verify(&id, "123456").await.is_err());
    assert_eq!(w.calls(SEND), 1);
    assert_eq!(w.calls(VERIFY), 0);
    w.client
        .login_command(
            &w.id,
            Cmd::SecurityMailStop {
                verification_id: id,
            },
        )
        .await
        .unwrap();
    assert!(w.client.login_snapshot(&w.id).unwrap().email.is_none());
}
#[tokio::test]
async fn lost_send_is_not_repeated_but_a_received_code_can_still_be_verified() {
    let w = World::new(
        true,
        true,
        [
            HttpResponse::Ready(Err(Error::Transport("synthetic lost send response".into()))),
            authorized(json!({"success":true})),
            identity(),
        ],
    )
    .await;
    assert!(
        w.client
            .login_command(&w.id, Cmd::SecurityMailSend)
            .await
            .is_err()
    );
    assert!(w.phase() == Phase::SendUnconfirmed);
    let id = w.client.login_snapshot(&w.id).unwrap().email.unwrap().id;
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::SecurityMailResend {
                    verification_id: id.clone()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        w.verify(&id, "123456").await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.calls(SEND), 1);
    assert_eq!(w.calls(VERIFY), 1);
}
#[tokio::test]
async fn unknown_verification_preserves_cookie_for_read_only_recovery_without_refresh_receipt() {
    let HttpResponse::Ready(Ok(mut response)) = authorized(json!({"success":true})) else {
        unreachable!()
    };
    response.body_status = BodyStatus::Interrupted;
    let w = World::new(
        true,
        true,
        [sent(), HttpResponse::Ready(Ok(response)), identity()],
    )
    .await;
    let id = w.start().await;
    assert!(w.verify(&id, "123456").await.is_err());
    assert!(w.phase() == Phase::RecoveryRequired);
    assert!(w.verify(&id, "123456").await.is_err());
    assert!(
        w.client
            .login_command(
                &w.id,
                Cmd::SecurityMailResend {
                    verification_id: id
                }
            )
            .await
            .is_err()
    );
    assert_eq!(
        w.client
            .login_command(&w.id, Cmd::Reconcile)
            .await
            .unwrap()
            .status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.calls(VERIFY), 1);
    let saved = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
        .unwrap()
        .current(Default::default())
        .unwrap()
        .unwrap()
        .credentials
        .login_credentials()
        .unwrap();
    assert!(saved.refresh_token.is_none() && saved.last_login_at.is_none());
}
#[tokio::test]
async fn local_stop_cancels_a_held_send_and_old_ids_cannot_control_the_replacement() {
    let (pending, mut response) = gate();
    let w = World::new(true, true, [pending, sent()]).await;
    let before = w.http.calls().len();
    let client = w.client.clone();
    let operation = w.id.clone();
    let task = tokio::spawn(async move {
        client
            .login_command(&operation, Cmd::SecurityMailSend)
            .await
    });
    w.http.wait(before + 1).await;
    let old = w.client.login_snapshot(&w.id).unwrap().email.unwrap().id;
    tokio::time::timeout(
        Duration::from_secs(1),
        w.client.login_command(
            &w.id,
            Cmd::SecurityMailStop {
                verification_id: old.clone(),
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), response.closed())
        .await
        .unwrap();
    assert!(task.await.unwrap().is_err());
    let next = w.start().await;
    assert_ne!(old, next);
    assert!(w.verify(&old, "123456").await.is_err());
    assert!(w.phase() == Phase::CodeRequired);
    assert_eq!(w.calls(SEND), 2);
    assert_eq!(w.calls(VERIFY), 0);
}
#[tokio::test]
async fn verified_step_keeps_replay_alive_until_response_or_explicit_cancel() {
    for cancel in [false, true] {
        let (pending, mut response) = gate();
        let w = World::new(
            true,
            true,
            [
                sent(),
                json!({"code":200}).into(),
                json!({"code":301}).into(),
                pending,
                identity(),
            ],
        )
        .await;
        let id = w.start().await;
        let before = w.http.calls().len();
        let client = w.client.clone();
        let operation = w.id.clone();
        let task = tokio::spawn(async move {
            client
                .login_command(
                    &operation,
                    Cmd::SecurityMailVerify {
                        verification_id: id,
                        code: "123456".into(),
                    },
                )
                .await
        });
        w.http.wait(before + 3).await;
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        assert!(w.client.login_snapshot(&w.id).unwrap().busy);
        if cancel {
            w.client.cancel_login(&w.id).unwrap();
            tokio::time::timeout(Duration::from_secs(1), response.closed())
                .await
                .unwrap();
            assert!(task.await.unwrap().is_err());
            assert_ne!(
                w.client.account().unwrap().status,
                SessionStatus::Authenticated
            );
        } else {
            let HttpResponse::Ready(reply) = authorized(json!({"success":true})) else {
                unreachable!()
            };
            assert!(response.send(reply).is_ok());
            assert_eq!(
                task.await.unwrap().unwrap().status,
                LoginStatus::Authenticated
            );
            assert_eq!(w.calls("/api/login"), 2);
        }
    }
}
#[tokio::test(start_paused = true)]
async fn timeouts_and_dropped_verification_tasks_do_not_reenable_unknown_writes() {
    for abort in [false, true] {
        let (pending, mut response) = gate();
        let w = World::new(true, true, [sent(), pending]).await;
        let id = w.start().await;
        let before = w.http.calls().len();
        let client = w.client.clone();
        let operation = w.id.clone();
        let old = id.clone();
        let task = tokio::spawn(async move {
            client
                .login_command(
                    &operation,
                    Cmd::SecurityMailVerify {
                        verification_id: old,
                        code: "123456".into(),
                    },
                )
                .await
        });
        w.http.wait(before + 1).await;
        if abort {
            task.abort();
        } else {
            tokio::time::advance(Duration::from_secs(46)).await;
        }
        let _ = task.await;
        response.closed().await;
        assert!(w.phase() == Phase::RecoveryRequired);
        assert!(w.verify(&id, "123456").await.is_err());
        assert_eq!(w.calls(VERIFY), 1);
    }
}

#[tokio::test]
async fn a_proven_not_sent_verification_can_retry_without_resending_email() {
    struct Once(AtomicBool);
    impl TokenProvider for Once {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            Box::pin(async move {
                if self.0.swap(false, Ordering::SeqCst) {
                    Err(Error::State("synthetic provider offline"))
                } else {
                    Ok("synthetic-device-proof".into())
                }
            })
        }
    }
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":[VERIFY]}]}}),
        ApiOrigin::Pc.as_str().into(),
        Arc::new(Once(AtomicBool::new(true))),
    ));
    let w = World::with_context(
        true,
        true,
        [sent(), authorized(json!({"success":true})), identity()],
        Some(context),
    )
    .await;
    let id = w.start().await;
    let error = w
        .verify(&id, "123456")
        .await
        .err()
        .expect("provider must fail before HTTP");
    assert!(matches!(error.root(), Error::RequestNotSent(_)));
    assert!(w.phase() == Phase::CodeRequired);
    assert_eq!(w.calls(VERIFY), 0);
    assert_eq!(
        w.verify(&id, "123456").await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.calls(SEND), 1);
    assert_eq!(w.calls(VERIFY), 1);
}
#[tokio::test]
async fn malformed_display_addresses_do_not_export_authorization_urls_or_block_the_pin_step() {
    for address in [
        "https://account.example/callback?token=DO_NOT_EXPORT".into(),
        "me@example.test\nDO_NOT_EXPORT".into(),
        format!("{}@example.test", "a".repeat(260)),
        String::new(),
    ] {
        let w = World::new(
            true,
            true,
            [json!({"code":200,"data":{"mailAddress":address}}).into()],
        )
        .await;
        let view = w
            .client
            .login_command(&w.id, Cmd::SecurityMailSend)
            .await
            .unwrap();
        let email = view.email.as_ref().unwrap();
        assert!(email.mail_address.is_none() && email.phase == Phase::CodeRequired);
        let raw = serde_json::to_string(&view).unwrap();
        assert!(!raw.contains("DO_NOT_EXPORT") && !raw.contains("https://account.example"));
    }
}
