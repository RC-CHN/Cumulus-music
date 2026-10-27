mod support;
use cumulus_core::{
    Error,
    account::{SessionStatus, login::LoginStatus, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode as Mode},
    },
    protocol::http::{ApiOrigin, BodyStatus},
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::http::{HttpResponse, ScriptHttp, gate, reply};

fn identity(uid: u64) -> HttpResponse {
    json!({"code":200,"profile":{"userId":uid,"nickname":format!("账号{uid}"),"avatarUrl":"http://p1.music.126.net/a.jpg?secret=private","private":"DO_NOT_EXPORT"}}).into()
}
fn authorized() -> HttpResponse {
    let mut r = reply(json!({"code":200,"private":"DO_NOT_EXPORT"}));
    r.set_cookies = vec!["MUSIC_U=synthetic-login; Path=/; Secure; HttpOnly".into()];
    r.refresh_token = Some("synthetic-refresh".into());
    HttpResponse::Ready(Ok(r))
}
fn password(remember: bool) -> Cmd {
    Cmd::Email {
        address: "me@example.test".into(),
        password: "synthetic-password".into(),
        remember,
    }
}
struct World {
    root: tempfile::TempDir,
    http: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
}

struct Proof(&'static str);
impl cumulus_core::client::LoginCaptchaProvider for Proof {
    fn solve(&self) -> cumulus_core::protocol::api::ApiFuture<'_, String> {
        Box::pin(async move { Ok(self.0.into()) })
    }
}
struct PendingProof {
    started: tokio::sync::Notify,
    receiver: std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<String>>>,
}
impl cumulus_core::client::LoginCaptchaProvider for PendingProof {
    fn solve(&self) -> cumulus_core::protocol::api::ApiFuture<'_, String> {
        Box::pin(async move {
            let receiver = self.receiver.lock().unwrap().take().unwrap();
            self.started.notify_one();
            receiver.await.map_err(|_| Error::Cancelled)
        })
    }
}

#[tokio::test]
async fn captcha_resubmits_only_pending_credentials_and_exports_no_proof() {
    let w = World::new([json!({"code":8821}).into(), authorized(), identity(2)]).await;
    let id = w.begin(Mode::Email).await;
    assert!(
        w.client
            .solve_login_captcha(&id, &Proof("unused"))
            .await
            .is_err()
    );
    w.client.login_command(&id, password(true)).await.unwrap();
    for invalid in ["", "bad\nproof"] {
        assert!(
            w.client
                .solve_login_captcha(&id, &Proof(invalid))
                .await
                .is_err()
        );
        assert_eq!(w.http.calls().len(), 2);
        assert_eq!(
            w.client.login_snapshot(&id).unwrap().status,
            LoginStatus::Captcha
        );
    }
    let result = w
        .client
        .solve_login_captcha(&id, &Proof("user-completed-proof"))
        .await
        .unwrap();
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["kind"], "credentials");
    assert_eq!(value["value"]["status"], "authenticated");
    assert!(!value.to_string().contains("user-completed-proof"));
    let calls = w.http.calls();
    assert_eq!(calls[1].path, calls[2].path);
    assert_eq!(calls[1].payload["password"], calls[2].payload["password"]);
    assert_eq!(calls[2].payload["secureCaptcha"], "user-completed-proof");
    assert_eq!(calls[3].path, "/api/w/nuser/account/get");
    assert!(
        w.client
            .solve_login_captcha(&id, &Proof("replay"))
            .await
            .is_err()
    );
    assert_eq!(w.http.calls().len(), 4);
}

#[tokio::test]
async fn captcha_cancel_and_replacement_drop_provider_without_any_login_replay() {
    for replace in [false, true] {
        let w = World::new([json!({"code":8821}).into()]).await;
        let id = w.begin(Mode::Email).await;
        w.client.login_command(&id, password(true)).await.unwrap();
        let (mut sender, receiver) = tokio::sync::oneshot::channel();
        let provider = Arc::new(PendingProof {
            started: Default::default(),
            receiver: std::sync::Mutex::new(Some(receiver)),
        });
        let task = {
            let client = w.client.clone();
            let id = id.clone();
            let provider = provider.clone();
            tokio::spawn(async move { client.solve_login_captcha(&id, provider.as_ref()).await })
        };
        provider.started.notified().await;
        assert!(w.client.login_snapshot(&id).unwrap().busy);
        assert!(
            w.client
                .solve_login_captcha(&id, &Proof("duplicate"))
                .await
                .is_err()
        );
        assert!(w.client.login_command(&id, Cmd::Reconcile).await.is_err());
        if replace {
            w.begin(Mode::Email).await;
        } else {
            w.client.cancel_login(&id).unwrap();
        }
        assert!(matches!(task.await.unwrap(), Err(Error::Cancelled)));
        sender.closed().await;
        assert_eq!(w.http.calls().len(), 2);
        assert_eq!(
            w.client.account().unwrap().nickname.as_deref(),
            Some("账号1")
        );
    }
}

#[tokio::test]
async fn interrupted_captcha_provider_releases_slot_and_same_challenge_can_retry() {
    let w = World::new([json!({"code":8821}).into(), authorized(), identity(2)]).await;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let (mut sender, receiver) = tokio::sync::oneshot::channel();
    let provider = Arc::new(PendingProof {
        started: Default::default(),
        receiver: std::sync::Mutex::new(Some(receiver)),
    });
    let task = {
        let client = w.client.clone();
        let id = id.clone();
        let provider = provider.clone();
        tokio::spawn(async move { client.solve_login_captcha(&id, provider.as_ref()).await })
    };
    provider.started.notified().await;
    task.abort();
    assert!(matches!(task.await, Err(e) if e.is_cancelled()));
    sender.closed().await;
    assert!(!w.client.login_snapshot(&id).unwrap().busy);
    assert_eq!(
        w.client.login_snapshot(&id).unwrap().status,
        LoginStatus::Captcha
    );
    assert_eq!(w.http.calls().len(), 2);
    w.client
        .solve_login_captcha(&id, &Proof("retry-completed"))
        .await
        .unwrap();
    assert_eq!(w.http.calls().len(), 4);
}

#[tokio::test]
async fn qr_captcha_keeps_original_key_and_adopts_only_after_identity_read() {
    let w = World::new([
        json!({"code":200,"unikey":"challenge-qr-key"}).into(),
        json!({"code":8821,"private":"never-export"}).into(),
        json!({"code":803}).into(),
        identity(2),
    ])
    .await;
    let qr = w.client.begin_qr().await.unwrap();
    assert_eq!(
        w.client.poll_qr(&qr.operation_id).await.unwrap(),
        cumulus_core::account::QrStatus::Challenge
    );
    let result = w
        .client
        .solve_login_captcha(&qr.operation_id, &Proof("qr-user-completed"))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({"kind":"qr","value":"authorized"})
    );
    let calls = w.http.calls();
    assert_eq!(calls[2].payload["key"], calls[3].payload["key"]);
    assert_eq!(calls[3].payload["secureCaptcha"], "qr-user-completed");
    assert_eq!(calls[4].path, "/api/w/nuser/account/get");
}

#[tokio::test(start_paused = true)]
async fn captcha_timeout_drops_window_provider_and_preserves_retryable_challenge() {
    let w = World::new([json!({"code":8821}).into()]).await;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let (mut sender, receiver) = tokio::sync::oneshot::channel();
    let provider = Arc::new(PendingProof {
        started: Default::default(),
        receiver: std::sync::Mutex::new(Some(receiver)),
    });
    let task = {
        let client = w.client.clone();
        let id = id.clone();
        let provider = provider.clone();
        tokio::spawn(async move { client.solve_login_captcha(&id, provider.as_ref()).await })
    };
    provider.started.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    assert!(matches!(
        task.await.unwrap(),
        Err(Error::State("CAPTCHA timed out"))
    ));
    sender.closed().await;
    let view = w.client.login_snapshot(&id).unwrap();
    assert_eq!(view.status, LoginStatus::Captcha);
    assert!(!view.busy);
    assert_eq!(w.http.calls().len(), 2);
}

#[tokio::test]
async fn captcha_does_not_invent_a_login_when_a_code_send_has_no_pending_credentials() {
    let w = World::new([json!({"code":8821}).into()]).await;
    let id = w.begin(Mode::PhoneSms).await;
    let view = w
        .client
        .login_command(
            &id,
            Cmd::SendSms {
                phone: "13800138000".into(),
                country_code: "86".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(view.status, LoginStatus::Captcha);
    assert!(!view.captcha_available);
    assert!(
        w.client
            .solve_login_captcha(&id, &Proof("must-not-send"))
            .await
            .is_err()
    );
    assert_eq!(w.http.calls().len(), 2);
}
impl World {
    async fn new(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let http = Arc::new(ScriptHttp::new([identity(1)].into_iter().chain(replies)));
        let client =
            Arc::new(MusicClient::open_with_http_backend(root.path(), None, http.clone()).unwrap());
        client.restore_account().await.unwrap();
        Self { root, http, client }
    }
    fn epoch(&self) -> String {
        self.client.account().unwrap().epoch
    }
    fn registry(&self) -> Arc<ProfileRegistry> {
        ProfileRegistry::open(self.root.path(), ApiOrigin::Pc).unwrap()
    }
    async fn begin(&self, mode: Mode) -> String {
        self.client
            .begin_login(&self.epoch(), mode, None)
            .await
            .unwrap()
            .operation_id
    }
}

#[tokio::test]
async fn verified_login_saves_safe_account_metadata_and_remember_controls_restart() {
    for remember in [true, false] {
        let w = World::new([authorized(), identity(2)]).await;
        let id = w.begin(Mode::Email).await;
        assert!(!w.client.account().unwrap().can_sign_out);
        let result = w
            .client
            .login_command(&id, password(remember))
            .await
            .unwrap();
        assert_eq!(result.status, LoginStatus::Authenticated);
        assert!(w.client.account().unwrap().can_sign_out);
        assert_eq!(
            w.client.account().unwrap().nickname.as_deref(),
            Some("账号2")
        );
        let saved = w.client.saved_accounts(&w.epoch()).unwrap();
        assert_eq!(saved.value.len(), 1);
        assert!(saved.value[0].current);
        assert_eq!(saved.value[0].auto_login, remember);
        assert_eq!(
            w.registry().selection().unwrap().unwrap().auto_login,
            remember
        );
        let json = serde_json::to_string(&saved).unwrap();
        for private in [
            "synthetic-password",
            "synthetic-login",
            "synthetic-refresh",
            "DO_NOT_EXPORT",
            "secret=private",
            "core.sqlite3",
            "userId",
        ] {
            assert!(!json.contains(private));
        }
        assert!(
            w.http.calls()[1]
                .cookie
                .as_deref()
                .is_none_or(|s| !s.contains("MUSIC_U"))
        );
        assert_eq!(
            w.http.calls()[1].payload["password"],
            cumulus_core::protocol::wire::eapi::md5_hex(b"synthetic-password")
        );
        let boot = Arc::new(ScriptHttp::new([if remember {
            identity(2)
        } else {
            json!({"code":301}).into()
        }]));
        let client =
            MusicClient::open_with_http_backend(w.root.path(), None, boot.clone()).unwrap();
        let restored = client.restore_account().await.unwrap();
        assert_eq!(restored.status == SessionStatus::Authenticated, remember);
        assert_eq!(
            boot.calls()[0]
                .cookie
                .as_deref()
                .is_some_and(|s| s.contains("MUSIC_U=synthetic-login")),
            remember
        );
        assert_eq!(
            client.saved_accounts(&restored.epoch).unwrap().value.len(),
            1
        );
    }
}

#[tokio::test]
async fn explicit_sms_send_and_shared_cooldown_survive_cancel_and_new_attempt() {
    let w = World::new([json!({"code":200}).into(), authorized(), identity(2)]).await;
    let id = w.begin(Mode::PhoneSms).await;
    let state = w
        .client
        .login_command(
            &id,
            Cmd::SendSms {
                phone: "13800138000".into(),
                country_code: "86".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(state.status, LoginStatus::CodeSent);
    assert!(state.retry_after_ms > 59000);
    w.client.cancel_login(&id).unwrap();
    let id = w.begin(Mode::PhoneSms).await;
    assert!(
        w.client
            .login_command(
                &id,
                Cmd::SendSms {
                    phone: "13800138000".into(),
                    country_code: "86".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.http.calls().len(), 2);
    w.client.cancel_login(&id).unwrap();
    let id = w.begin(Mode::PhoneSms).await;
    let state = w
        .client
        .login_command(
            &id,
            Cmd::PhoneSms {
                phone: "13800138000".into(),
                country_code: "86".into(),
                code: "123456".into(),
                remember: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(state.status, LoginStatus::Authenticated);
    let calls = w.http.calls();
    assert_eq!(calls[2].payload["captcha"], "123456");
    assert!(calls[2].payload.get("password").is_none());
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.path == "/api/sms/captcha/sent")
            .count(),
        1
    );
}

#[tokio::test]
async fn cancelled_pending_login_does_not_read_identity_or_replace_current_account() {
    let (pending, release) = gate();
    let w = World::new([pending]).await;
    let id = w.begin(Mode::Email).await;
    let task = {
        let c = w.client.clone();
        let id = id.clone();
        tokio::spawn(async move { c.login_command(&id, password(true)).await })
    };
    w.http.wait(2).await;
    w.client.cancel_login(&id).unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号1")
    );
    assert!(w.registry().saved().unwrap().is_empty());
    assert_eq!(w.http.calls().len(), 2);
}

#[tokio::test]
async fn interrupted_login_reconciles_candidate_identity_without_replaying_password() {
    let mut interrupted = reply(json!({"code":200}));
    interrupted.set_cookies = vec!["MUSIC_U=synthetic-lost-reply; Path=/; Secure; HttpOnly".into()];
    interrupted.body_status = BodyStatus::Interrupted;
    let w = World::new([HttpResponse::Ready(Ok(interrupted)), identity(2)]).await;
    let id = w.begin(Mode::Email).await;
    assert!(w.client.login_command(&id, password(true)).await.is_err());
    assert!(w.client.login_snapshot(&id).unwrap().interrupted);
    assert!(w.client.login_command(&id, password(true)).await.is_err());
    assert_eq!(w.http.calls().len(), 2);
    assert_eq!(
        w.client
            .login_command(&id, Cmd::Reconcile)
            .await
            .unwrap()
            .status,
        LoginStatus::Authenticated
    );
    let calls = w.http.calls();
    assert_eq!(calls[2].path, "/api/w/nuser/account/get");
    assert!(
        calls[2]
            .cookie
            .as_deref()
            .unwrap()
            .contains("synthetic-lost-reply")
    );
}

#[tokio::test]
async fn saved_account_switch_reads_identity_and_rejects_another_uid_without_replacement() {
    let w = World::new([
        authorized(),
        identity(2),
        authorized(),
        identity(3),
        identity(4),
        identity(2),
    ])
    .await;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let old = w
        .client
        .saved_accounts(&w.epoch())
        .unwrap()
        .value
        .remove(0)
        .id;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let selected = w.registry().selection().unwrap();
    let id = w
        .client
        .begin_login(&w.epoch(), Mode::SavedAccount, Some(&old))
        .await
        .unwrap()
        .operation_id;
    assert!(matches!(
        w.client.login_command(&id, Cmd::Reconcile).await,
        Err(Error::Response("saved account identity changed"))
    ));
    assert_eq!(w.registry().selection().unwrap(), selected);
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号3")
    );
    w.client.cancel_login(&id).unwrap();
    let id = w
        .client
        .begin_login(&w.epoch(), Mode::SavedAccount, Some(&old))
        .await
        .unwrap()
        .operation_id;
    w.client.login_command(&id, Cmd::Reconcile).await.unwrap();
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号2")
    );
    assert_eq!(w.registry().saved().unwrap().len(), 2);
    assert_eq!(
        w.http
            .calls()
            .iter()
            .filter(|c| c.path == "/api/w/login")
            .count(),
        2
    );
}

#[tokio::test]
async fn metadata_failure_cannot_publish_selection_or_current_account_and_can_be_reconciled() {
    let w = World::new([authorized(), identity(2), identity(2)]).await;
    let id = w.begin(Mode::Email).await;
    let db = rusqlite::Connection::open(w.root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_saved BEFORE INSERT ON state WHEN NEW.kind='saved-profiles-v1' BEGIN SELECT RAISE(FAIL,'synthetic full disk'); END;").unwrap();
    assert!(w.client.login_command(&id, password(true)).await.is_err());
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("账号1")
    );
    assert!(w.registry().selection().unwrap().is_none());
    assert!(w.registry().saved().unwrap().is_empty());
    db.execute_batch("DROP TRIGGER reject_saved;").unwrap();
    w.client.login_command(&id, Cmd::Reconcile).await.unwrap();
    assert_eq!(w.registry().saved().unwrap().len(), 1);
    assert_eq!(
        w.http
            .calls()
            .iter()
            .filter(|c| c.path == "/api/w/login")
            .count(),
        1
    );
}

#[tokio::test]
async fn two_factor_offers_only_whitelisted_methods_and_answers_reconcile_before_one_replay() {
    let w=World::new([
        json!({"code":8830,"MUSIC_TU":"private-ticket","secret":"DO_NOT_EXPORT"}).into(),
        json!({"code":200,"data":{"types":["SECURITY_EMAIL","NOT_A_METHOD"],"nickName":"目标账号","phoneMask":"138****8000","MUSIC_U":"DO_NOT_EXPORT"}}).into(),
        json!({"code":200}).into(), json!({"code":200}).into(),
        json!({"code":200}).into(), json!({"code":301}).into(), authorized(),identity(2),
    ]).await;
    let id = w.begin(Mode::Email).await;
    assert_eq!(
        w.client
            .login_command(&id, password(true))
            .await
            .unwrap()
            .status,
        LoginStatus::TwoFactor
    );
    assert!(
        w.client
            .login_command(&id, Cmd::SecurityMailSend)
            .await
            .is_err()
    );
    let view = w
        .client
        .login_command(&id, Cmd::VerificationMethods)
        .await
        .unwrap();
    assert_eq!(view.challenge.methods, vec!["SECURITY_EMAIL"]);
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("DO_NOT_EXPORT")
    );
    assert!(
        w.client
            .login_command(&id, Cmd::VerificationSmsSend)
            .await
            .is_err()
    );
    w.client
        .login_command(&id, Cmd::SecurityMailSend)
        .await
        .unwrap();
    let done = w
        .client
        .login_command(
            &id,
            Cmd::SecurityMailVerify {
                code: "123456".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(done.status, LoginStatus::Authenticated);
    let calls = w.http.calls();
    assert_eq!(calls.iter().filter(|c| c.path == "/api/w/login").count(), 2);
    assert_eq!(calls[5].path, "/api/user/2fa/securityEmail/verify");
    assert_eq!(calls[6].path, "/api/w/nuser/account/get");
}

#[tokio::test]
async fn official_question_options_and_none_answer_are_projected_without_raw_challenge() {
    let w=World::new([
        json!({"code":8861}).into(),
        json!({"code":200,"data":{"title":"使用过哪个设备？","questionType":1,"options":[{"title":"设备甲","subtitle":"最近使用","private":"DO_NOT_EXPORT"}]}}).into(),
        json!({"code":200}).into(),identity(2),
    ]).await;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let q = w
        .client
        .login_command(&id, Cmd::QuestionRead)
        .await
        .unwrap();
    assert_eq!(q.challenge.answers[0].value, "设备甲");
    assert_eq!(q.challenge.answers[1].value, "none");
    assert!(
        w.client
            .login_command(
                &id,
                Cmd::QuestionAnswer {
                    answer: "injected".into()
                }
            )
            .await
            .is_err()
    );
    w.client
        .login_command(
            &id,
            Cmd::QuestionAnswer {
                answer: "none".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(w.http.calls()[3].payload["ans"], "none");
}

#[tokio::test]
async fn logout_clears_only_bound_credentials_and_failed_remote_logout_keeps_session() {
    let w = World::new([
        authorized(),
        identity(2),
        HttpResponse::Ready(Err(Error::Transport("lost logout reply".into()))),
    ])
    .await;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let epoch = w.epoch();
    assert!(w.client.logout_account(&epoch, true).await.is_err());
    assert_eq!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    let done = w.client.logout_account(&epoch, false).await.unwrap();
    assert!(!done.remote_revoked);
    assert_eq!(done.account.status, SessionStatus::SignedOut);
    assert!(!done.account.can_sign_out);
    let selected = w.registry().current(Default::default()).unwrap().unwrap();
    assert!(
        selected
            .credentials
            .login_credentials()
            .unwrap()
            .token
            .is_none()
    );
    assert_eq!(w.registry().saved().unwrap().len(), 1);
    assert!(matches!(
        w.client.logout_account(&epoch, false).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        w.http
            .calls()
            .iter()
            .filter(|c| c.path == "/api/logout")
            .count(),
        1
    );
}

#[tokio::test]
async fn invalid_epoch_mode_and_credentials_do_not_send_requests() {
    let w = World::new([]).await;
    assert!(matches!(
        w.client.begin_login("0", Mode::Email, None).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        w.client
            .begin_login(&w.epoch(), Mode::SavedAccount, Some("../../outside"))
            .await
            .is_err()
    );
    let id = w.begin(Mode::PhonePassword).await;
    assert!(w.client.login_command(&id, password(true)).await.is_err());
    assert!(
        w.client
            .login_command(
                &id,
                Cmd::PhonePassword {
                    phone: "bad".into(),
                    country_code: "86".into(),
                    password: "p".into(),
                    remember: true
                }
            )
            .await
            .is_err()
    );
    assert_eq!(w.http.calls().len(), 1);
    assert!(serde_json::from_value::<Cmd>(json!({"kind":"email","address":"a@b","password":"p","remember":true,"endpoint":"https://arbitrary"})).is_err());
    let _: Value = serde_json::to_value(w.client.login_snapshot(&id).unwrap()).unwrap();
}

#[tokio::test]
async fn unresolved_room_creation_blocks_login_even_without_a_local_room_binding() {
    let w = World::new([]).await;
    let store = w.registry().store();
    store
        .put(
            "1",
            "listen-together-operations-v1",
            "uncertain",
            &json!({
                "id":"uncertain","intent":{"kind":"create","refer":"fixture"},
                "phase":"uncertain","acknowledged":false,"created_room":null,"observed_room":null
            }),
        )
        .unwrap();
    assert!(matches!(
        w.client.begin_login(&w.epoch(), Mode::Email, None).await,
        Err(Error::State("leave together before switching accounts"))
    ));
    assert!(matches!(
        w.client.begin_qr().await,
        Err(Error::State("leave together before switching accounts"))
    ));
    assert!(matches!(
        w.client.logout_account(&w.epoch(), false).await,
        Err(Error::State("leave together before switching accounts"))
    ));
    assert_eq!(w.http.calls().len(), 1);
    store
        .delete("1", "listen-together-operations-v1", "uncertain")
        .unwrap();
    w.begin(Mode::Email).await;
}

#[tokio::test]
async fn empty_security_question_reconciles_identity_before_a_single_acknowledged_replay() {
    let w = World::new([
        json!({"code":8861}).into(),
        json!({"code":200,"data":{}}).into(),
        json!({"code":301}).into(),
        authorized(),
        identity(2),
    ])
    .await;
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, password(true)).await.unwrap();
    let result = w
        .client
        .login_command(&id, Cmd::QuestionRead)
        .await
        .unwrap();
    assert_eq!(result.status, LoginStatus::Authenticated);
    assert_eq!(
        w.http
            .calls()
            .iter()
            .filter(|c| c.path == "/api/w/login")
            .count(),
        2
    );
}
