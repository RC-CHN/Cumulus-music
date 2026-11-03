mod support;
use cumulus_core::{
    Error, Result,
    account::{
        SessionStatus,
        login::{
            LoginStatus, NicknameError, RegistrationBlocked, ReuseChoice, ReusePhase as Phase,
        },
        profiles::ProfileRegistry,
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode, LoginView},
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, BodyStatus},
    },
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use support::http::{HttpResponse, ScriptHttp, gate, reply};
const READ: &str = "/api/login/cellphone/twice/used/user/get";
const CONFIRM: &str = "/api/login/cellphone/twice/used/confirm";
const HINT: &str = "/api/nickname/hint/get";
const DUP: &str = "/api/nickname/duplicated";
const REGISTER: &str = "/api/w/register/cellphone";
fn info() -> HttpResponse {
    json!({"code":200,"data":{"nickName":"合成关联账号","createTime":"2021-03-04","avatarImgUrl":"","avatarUrl":"http://p1.music.126.net/a.jpg?private=DO_NOT_EXPORT","phoneNumber":"DO_NOT_EXPORT"}}).into()
}
fn hint() -> HttpResponse {
    json!({"code":200,"hintNickname":"合成昵称"}).into()
}
fn available() -> HttpResponse {
    json!({"code":200,"duplicated":false}).into()
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2,"nickname":"合成注册账号"}}).into()
}
fn authorized() -> HttpResponse {
    let mut r = reply(json!({"code":200}));
    r.set_cookies = vec!["MUSIC_U=synthetic-reuse; Path=/; Secure; HttpOnly".into()];
    r.refresh_token = Some("synthetic-reuse-refresh".into());
    HttpResponse::Ready(Ok(r))
}
struct World {
    root: tempfile::TempDir,
    client: Arc<MusicClient>,
    http: Arc<ScriptHttp>,
    id: String,
}
impl World {
    async fn new(jsk: bool, replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self::context(jsk, true, true, replies, None).await
    }
    async fn context(
        jsk: bool,
        sms: bool,
        skip: bool,
        replies: impl IntoIterator<Item = HttpResponse>,
        context: Option<Arc<RequestContext>>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut all = vec![];
        if jsk {
            all.push(json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into());
        }
        all.push(json!({"code":8860,"canSkipSetNickname":skip}).into());
        all.extend(replies);
        let http = Arc::new(ScriptHttp::new(all));
        let client = MusicClient::open_with_request_context(
            root.path(),
            None,
            Default::default(),
            Some(http.clone()),
            context,
        )
        .unwrap();
        let client = Arc::new(if jsk {
            client.with_official_login_policy()
        } else {
            client
        });
        let id = start(&client, sms).await;
        Self {
            root,
            client,
            http,
            id,
        }
    }
    async fn run(&self, c: Cmd) -> Result<LoginView> {
        self.client.login_command(&self.id, c).await
    }
    async fn read(&self) -> String {
        self.run(Cmd::ReuseRead).await.unwrap().reuse.unwrap().id
    }
    async fn choose(&self, id: &str, choice: ReuseChoice) -> LoginView {
        self.run(Cmd::ReuseChoose {
            verification_id: id.into(),
            choice,
        })
        .await
        .unwrap()
    }
    async fn prepare(&self) -> String {
        let id = self.read().await;
        self.choose(&id, ReuseChoice::Register).await;
        self.run(Cmd::ReusePrepareRegister {
            verification_id: id.clone(),
        })
        .await
        .unwrap();
        id
    }
    async fn register(&self, id: &str, name: Option<&str>) -> Result<LoginView> {
        self.run(Cmd::ReuseRegister {
            verification_id: id.into(),
            nickname: name.map(str::to_owned),
        })
        .await
    }
    fn count(&self, path: &str) -> usize {
        self.http.calls().iter().filter(|c| c.path == path).count()
    }
    fn pending(&self) -> usize {
        ProfileRegistry::open(self.root.path(), ApiOrigin::Pc)
            .unwrap()
            .store()
            .values("transport", "phone-registration-v1")
            .unwrap()
            .len()
    }
    fn phase(&self) -> Phase {
        self.client
            .login_snapshot(&self.id)
            .unwrap()
            .reuse
            .unwrap()
            .phase
    }
}
async fn start(client: &MusicClient, sms: bool) -> String {
    let id = client
        .begin_login(
            &client.account().unwrap().epoch,
            if sms {
                LoginMode::PhoneSms
            } else {
                LoginMode::PhonePassword
            },
            None,
        )
        .await
        .unwrap()
        .operation_id;
    let command = if sms {
        Cmd::PhoneSms {
            phone: "13800000000".into(),
            country_code: "86".into(),
            code: "246810".into(),
            remember: false,
        }
    } else {
        Cmd::PhonePassword {
            phone: "13800000000".into(),
            country_code: "86".into(),
            password: "synthetic-password".into(),
            remember: false,
        }
    };
    client.login_command(&id, command).await.unwrap();
    id
}
#[tokio::test]
async fn both_protocols_require_a_native_two_step_choice_before_confirm_and_preserve_auto_login() {
    for jsk in [false, true] {
        let w = World::new(jsk, [info(), authorized(), identity()]).await;
        assert!(
            w.run(Cmd::ReuseConfirm {
                verification_id: "injected".into()
            })
            .await
            .is_err()
        );
        let id = w.read().await;
        let view = w.client.login_snapshot(&w.id).unwrap();
        assert!(view.reuse.as_ref().unwrap().phase == Phase::Choice);
        assert_eq!(
            view.reuse.as_ref().unwrap().masked_nickname.as_deref(),
            Some("合****")
        );
        assert_eq!(
            view.reuse.as_ref().unwrap().created_at,
            Some(json!("2021-03-04"))
        );
        assert!(
            !serde_json::to_string(&view)
                .unwrap()
                .contains("DO_NOT_EXPORT")
        );
        assert!(
            w.run(Cmd::ReuseConfirm {
                verification_id: id.clone()
            })
            .await
            .is_err()
        );
        let before = w.http.calls().len();
        let choice = w.choose(&id, ReuseChoice::Login).await;
        assert!(choice.reuse.unwrap().phase == Phase::LoginConsent);
        assert_eq!(before, w.http.calls().len());
        assert_eq!(
            w.run(Cmd::ReuseConfirm {
                verification_id: id
            })
            .await
            .unwrap()
            .status,
            LoginStatus::Authenticated
        );
        let registry = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc).unwrap();
        let selected = registry.current(Default::default()).unwrap().unwrap();
        assert!(!selected.selection.auto_login);
        let credentials = selected.credentials.login_credentials().unwrap();
        assert_eq!(
            credentials.refresh_token.as_deref(),
            Some("synthetic-reuse-refresh")
        );
        assert_eq!(credentials.last_login_at.is_some(), jsk);
        let official: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/reuse-login-official.json")).unwrap();
        for (path, label) in [(READ, "Read"), (CONFIRM, "Confirm")] {
            let c = w.http.calls().into_iter().find(|c| c.path == path).unwrap();
            let vector =
                &official["vectors"][format!("{}{label}", if jsk { "jsk" } else { "legacy" })];
            assert_eq!(vector["path"], path);
            for (k, v) in vector["payload"].as_object().unwrap() {
                assert_eq!(&c.payload[k], v);
            }
            assert_eq!(c.payload.get("fromPage").is_some(), jsk);
            assert_eq!(c.login_chain.is_some(), jsk);
        }
    }
}
#[tokio::test]
async fn consent_cancel_rereads_user_info_and_invalidates_the_old_id_without_any_confirm_write() {
    let w = World::new(true, [info(), info()]).await;
    let old = w.read().await;
    w.choose(&old, ReuseChoice::Login).await;
    let next = w
        .run(Cmd::ReuseBack {
            verification_id: old.clone(),
        })
        .await
        .unwrap()
        .reuse
        .unwrap();
    assert!(next.phase == Phase::Choice);
    assert_ne!(next.id, old);
    assert!(
        w.run(Cmd::ReuseChoose {
            verification_id: old,
            choice: ReuseChoice::Register
        })
        .await
        .is_err()
    );
    assert_eq!(w.count(READ), 2);
    assert_eq!(w.count(CONFIRM), 0);
    assert_eq!(w.count(REGISTER), 0);
}
#[tokio::test]
async fn registration_uses_original_sms_and_actual_pc_legacy_route_even_after_jsk() {
    for jsk in [false, true] {
        for default in [false, true] {
            let mut replies = vec![info(), hint()];
            if !default {
                replies.push(available());
            }
            replies.extend([authorized(), identity()]);
            let w = World::new(jsk, replies).await;
            let id = w.prepare().await;
            assert_eq!(w.count(REGISTER), 0);
            assert_eq!(w.count(CONFIRM), 0);
            assert_eq!(
                w.register(
                    &id,
                    if default {
                        None
                    } else {
                        Some("  社区昵称  ")
                    }
                )
                .await
                .unwrap()
                .status,
                LoginStatus::Authenticated
            );
            assert_eq!(w.pending(), 0);
            let calls = w.http.calls();
            let call = calls.iter().find(|c| c.path == REGISTER).unwrap();
            let official: serde_json::Value =
                serde_json::from_str(include_str!("fixtures/reuse-login-official.json")).unwrap();
            let expected = &official["vectors"][if default {
                "registerDefault"
            } else {
                "registerNamed"
            }]["payload"];
            for (k, v) in expected.as_object().unwrap() {
                assert_eq!(&call.payload[k], v);
            }
            for key in ["fromPage", "ursToken", "once", "nonce", "secureCaptcha"] {
                assert!(call.payload.get(key).is_none());
            }
            assert!(
                call.payload
                    .get(if default {
                        "nickname"
                    } else {
                        "isSkipSetNickname"
                    })
                    .is_none()
            );
            assert!(call.login_chain.is_none() && call.music_tu.is_none());
            for c in calls.iter().filter(|c| c.path == HINT || c.path == DUP) {
                assert!(c.login_chain.is_none() && c.payload.get("fromPage").is_none());
            }
            let selected = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
                .unwrap()
                .current(Default::default())
                .unwrap()
                .unwrap();
            assert!(!selected.selection.auto_login);
            assert!(
                selected
                    .credentials
                    .login_credentials()
                    .unwrap()
                    .last_login_at
                    .is_none()
            );
        }
    }
}
#[tokio::test]
async fn nickname_validation_duplication_and_offered_default_are_enforced_before_registration() {
    let w = World::context(
        true,
        true,
        false,
        [
            info(),
            hint(),
            json!({"code":200,"duplicated":true}).into(),
            HttpResponse::Ready(Err(Error::Transport(
                "synthetic duplicate-read loss".into(),
            ))),
        ],
        None,
    )
    .await;
    let id = w.prepare().await;
    for (name, error) in [
        ("a", NicknameError::TooShort),
        ("a!aa", NicknameError::InvalidCharacters),
        ("😀😀", NicknameError::InvalidCharacters),
        ("😀😀😀😀😀😀😀😀", NicknameError::TooLong),
        ("abcdefghijklmnopqrstuvwxyz12345", NicknameError::TooLong),
    ] {
        let v = w.register(&id, Some(name)).await.unwrap().reuse.unwrap();
        assert!(v.phase == Phase::Nickname && v.nickname_error == Some(error));
    }
    assert_eq!(w.count(DUP), 0);
    assert!(w.register(&id, None).await.is_err());
    assert!(
        w.register(&id, Some("重复昵称"))
            .await
            .unwrap()
            .reuse
            .unwrap()
            .nickname_error
            == Some(NicknameError::Taken)
    );
    assert!(
        w.register(&id, Some("可用昵称"))
            .await
            .unwrap()
            .reuse
            .unwrap()
            .nickname_error
            == Some(NicknameError::CheckFailed)
    );
    assert_eq!(w.count(REGISTER), 0);
    assert_eq!(w.pending(), 0);
}

#[tokio::test]
async fn an_old_candidate_cookie_cannot_resolve_registration_without_an_applicable_response_receipt()
 {
    for cookie in [
        None,
        Some("MUSIC_U=synthetic-reuse; Domain=unrelated.test; Path=/; Secure"),
        Some("MUSIC_U=synthetic-reuse; Path=/other; Secure"),
        Some("MUSIC_U=synthetic-reuse; Max-Age=0; Path=/; Secure"),
    ] {
        let mut old = reply(json!({"code":200,"data":{"nickName":"合成关联账号"}}));
        old.set_cookies = vec!["MUSIC_U=synthetic-reuse; Path=/; Secure; HttpOnly".into()];
        let mut registration = reply(json!({"code":200}));
        registration.set_cookies = cookie.map(str::to_owned).into_iter().collect();
        let w = World::new(
            false,
            [
                HttpResponse::Ready(Ok(old)),
                hint(),
                HttpResponse::Ready(Ok(registration)),
            ],
        )
        .await;
        let id = w.prepare().await;
        assert!(w.register(&id, None).await.is_err());
        assert!(w.phase() == Phase::RecoveryRequired);
        assert_eq!(w.pending(), 1);
        assert!(w.run(Cmd::Reconcile).await.is_err());
        assert_eq!(w.count("/api/w/nuser/account/get"), 0);
        assert_ne!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
    }
}
#[tokio::test]
async fn failed_profile_selection_rolls_back_registration_settlement_and_can_reconcile_after_storage_recovers()
 {
    let w = World::new(
        false,
        [info(), hint(), authorized(), identity(), identity()],
    )
    .await;
    let id = w.prepare().await;
    let db = rusqlite::Connection::open(w.root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_registration_adoption BEFORE INSERT ON state WHEN NEW.kind='saved-profiles-v1' BEGIN SELECT RAISE(ABORT,'synthetic adoption failure'); END;").unwrap();
    assert!(w.register(&id, None).await.is_err());
    assert_eq!(w.pending(), 1);
    assert_ne!(
        w.client.account().unwrap().status,
        SessionStatus::Authenticated
    );
    assert!(
        ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
            .unwrap()
            .selection()
            .unwrap()
            .is_none()
    );
    db.execute_batch("DROP TRIGGER reject_registration_adoption;")
        .unwrap();
    assert_eq!(
        w.run(Cmd::Reconcile).await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.pending(), 0);
    assert_eq!(w.count(REGISTER), 1);
}

#[tokio::test]
async fn official_registration_errors_are_distinct_and_invalid_sms_cannot_be_resubmitted() {
    for (code, error) in [
        (503, NicknameError::SmsInvalid),
        (504, NicknameError::PhoneRegistered),
        (250, NicknameError::AntiAbuse),
        (509, NicknameError::RateLimited),
    ] {
        let w = World::new(false, [info(), hint(), json!({"code":code}).into()]).await;
        let id = w.prepare().await;
        let v = w.register(&id, None).await.unwrap().reuse.unwrap();
        assert!(v.nickname_error == Some(error));
        assert_eq!(w.pending(), 0);
        if code == 503 {
            assert!(w.register(&id, None).await.is_err());
            assert_eq!(w.count(REGISTER), 1);
        }
    }
}

#[tokio::test]
async fn unsent_login_replay_after_accepted_confirmation_never_reoffers_the_confirm_write() {
    struct Second(std::sync::atomic::AtomicUsize);
    impl TokenProvider for Second {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            Box::pin(async move {
                if self.0.fetch_add(1, Ordering::SeqCst) == 1 {
                    Err(Error::State("synthetic replay preparation failure"))
                } else {
                    Ok("synthetic-proof".into())
                }
            })
        }
    }
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":["/api/login/cellphone"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        Arc::new(Second(std::sync::atomic::AtomicUsize::new(0))),
    ));
    let w = World::context(
        true,
        true,
        true,
        [
            info(),
            json!({"code":200}).into(),
            json!({"code":301}).into(),
        ],
        Some(context),
    )
    .await;
    let id = w.read().await;
    w.choose(&id, ReuseChoice::Login).await;
    assert!(matches!(
        w.run(Cmd::ReuseConfirm {
            verification_id: id.clone()
        })
        .await
        .err()
        .unwrap()
        .root(),
        Error::RequestNotSent(_)
    ));
    assert!(
        w.phase() == Phase::RecoveryRequired,
        "accepted confirmation must never be offered again"
    );
    assert!(
        w.run(Cmd::ReuseConfirm {
            verification_id: id
        })
        .await
        .is_err()
    );
    assert_eq!(w.count(CONFIRM), 1);
    assert_eq!(w.count("/api/login/cellphone"), 1);
}

#[tokio::test]
async fn a_proved_unsent_confirmation_itself_remains_explicitly_retryable_in_both_protocols() {
    struct Once(AtomicBool);
    impl TokenProvider for Once {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            Box::pin(async move {
                if self.0.swap(false, Ordering::SeqCst) {
                    Err(Error::State("synthetic confirmation preparation failure"))
                } else {
                    Ok("synthetic-proof".into())
                }
            })
        }
    }
    for jsk in [false, true] {
        let context = Arc::new(RequestContext::new(
            json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":[CONFIRM]}]}}),
            ApiOrigin::Pc.as_str().into(),
            Arc::new(Once(AtomicBool::new(true))),
        ));
        let w = World::context(
            jsk,
            true,
            true,
            [info(), authorized(), identity()],
            Some(context),
        )
        .await;
        let id = w.read().await;
        w.choose(&id, ReuseChoice::Login).await;
        assert!(matches!(
            w.run(Cmd::ReuseConfirm {
                verification_id: id.clone()
            })
            .await
            .err()
            .unwrap()
            .root(),
            Error::RequestNotSent(_)
        ));
        assert!(w.phase() == Phase::LoginFailed);
        assert_eq!(w.count(CONFIRM), 0);
        assert_eq!(
            w.run(Cmd::ReuseConfirm {
                verification_id: id
            })
            .await
            .unwrap()
            .status,
            LoginStatus::Authenticated
        );
        assert_eq!(w.count(CONFIRM), 1);
    }
}
#[tokio::test]
async fn password_challenge_can_confirm_login_but_cannot_register_without_a_fresh_sms() {
    let w = World::context(false, false, true, [info()], None).await;
    let id = w.read().await;
    assert!(
        w.client
            .login_snapshot(&w.id)
            .unwrap()
            .reuse
            .unwrap()
            .registration_blocked
            == Some(RegistrationBlocked::FreshSmsRequired)
    );
    w.choose(&id, ReuseChoice::Register).await;
    assert!(
        w.run(Cmd::ReusePrepareRegister {
            verification_id: id
        })
        .await
        .is_err()
    );
    assert_eq!(w.count(HINT), 0);
    assert_eq!(w.count(REGISTER), 0);
}
#[tokio::test]
async fn accepted_registration_does_not_replay_sms_or_force_when_identity_is_unconfirmed() {
    let w = World::new(
        true,
        [
            info(),
            hint(),
            authorized(),
            json!({"code":301}).into(),
            identity(),
        ],
    )
    .await;
    let id = w.prepare().await;
    assert!(w.register(&id, None).await.is_err());
    assert!(w.phase() == Phase::RecoveryRequired);
    assert_eq!(w.pending(), 1);
    assert!(w.register(&id, None).await.is_err());
    assert_eq!(w.count(REGISTER), 1);
    assert_eq!(w.count("/api/login/cellphone"), 1);
    assert_eq!(
        w.run(Cmd::Reconcile).await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.pending(), 0);
}
#[tokio::test]
async fn unknown_registration_is_durable_across_candidates_and_restart_until_fresh_sms_identity() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            [
                info(),
                hint(),
                HttpResponse::Ready(Err(Error::Transport("synthetic register loss".into()))),
            ],
        )
        .await;
        let id = w.prepare().await;
        assert!(w.register(&id, None).await.is_err());
        assert_eq!(w.pending(), 1);
        let http = Arc::new(ScriptHttp::new([json!({"code":8860}).into(), info()]));
        let client =
            MusicClient::open_with_http_backend(w.root.path(), None, http.clone()).unwrap();
        let op = start(&client, true).await;
        let v = client
            .login_command(&op, Cmd::ReuseRead)
            .await
            .unwrap()
            .reuse
            .unwrap();
        assert!(v.registration_blocked == Some(RegistrationBlocked::UnconfirmedRegistration));
        client
            .login_command(
                &op,
                Cmd::ReuseChoose {
                    verification_id: v.id.clone(),
                    choice: ReuseChoice::Register,
                },
            )
            .await
            .unwrap();
        assert!(
            client
                .login_command(
                    &op,
                    Cmd::ReusePrepareRegister {
                        verification_id: v.id
                    }
                )
                .await
                .is_err()
        );
        assert_eq!(http.calls().len(), 2);
        drop(client);
        let http = Arc::new(ScriptHttp::new([authorized(), identity()]));
        let client = MusicClient::open_with_http_backend(w.root.path(), None, http).unwrap();
        start(&client, true).await;
        assert_eq!(
            client.account().unwrap().status,
            SessionStatus::Authenticated
        );
        assert_eq!(w.pending(), 0);
        let rows = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
            .unwrap()
            .store()
            .values("transport", "phone-registration-v1")
            .unwrap();
        assert!(rows.is_empty());
    }
}

#[tokio::test]
async fn an_older_inflight_sms_login_cannot_clear_a_newer_candidates_unresolved_registration() {
    let w = World::new(
        false,
        [
            info(),
            hint(),
            HttpResponse::Ready(Err(Error::Transport("synthetic registration loss".into()))),
        ],
    )
    .await;
    let (pending, complete) = gate();
    let http = Arc::new(ScriptHttp::new([pending, identity()]));
    let other =
        Arc::new(MusicClient::open_with_http_backend(w.root.path(), None, http.clone()).unwrap());
    let clone = other.clone();
    let task = tokio::spawn(async move { start(&clone, true).await });
    http.wait(1).await;
    let id = w.prepare().await;
    assert!(w.register(&id, None).await.is_err());
    assert_eq!(w.pending(), 1);
    let HttpResponse::Ready(response) = authorized() else {
        unreachable!()
    };
    assert!(complete.send(response).is_ok());
    task.await.unwrap();
    assert_eq!(
        other.account().unwrap().status,
        SessionStatus::Authenticated
    );
    assert_eq!(w.pending(), 1);
    let fresh = MusicClient::open_with_http_backend(
        w.root.path(),
        None,
        Arc::new(ScriptHttp::new([authorized(), identity()])),
    )
    .unwrap();
    start(&fresh, true).await;
    assert_eq!(w.pending(), 0);
}
#[tokio::test]
async fn known_rejection_and_proved_unsent_registration_release_only_their_own_intent() {
    struct Once(AtomicBool);
    impl TokenProvider for Once {
        fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
            Box::pin(async move {
                if self.0.swap(false, Ordering::SeqCst) {
                    Err(Error::State("synthetic preflight failure"))
                } else {
                    Ok("synthetic-proof".into())
                }
            })
        }
    }
    for jsk in [false, true] {
        let context = Arc::new(RequestContext::new(
            json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":[REGISTER]}]}}),
            ApiOrigin::Pc.as_str().into(),
            Arc::new(Once(AtomicBool::new(true))),
        ));
        let w = World::context(
            jsk,
            true,
            true,
            [
                info(),
                hint(),
                json!({"code":509}).into(),
                authorized(),
                identity(),
            ],
            Some(context),
        )
        .await;
        let id = w.prepare().await;
        assert!(matches!(
            w.register(&id, None).await.err().unwrap().root(),
            Error::RequestNotSent(_)
        ));
        assert_eq!(w.count(REGISTER), 0);
        assert_eq!(w.pending(), 0);
        assert!(w.phase() == Phase::Nickname);
        assert!(
            w.register(&id, None)
                .await
                .unwrap()
                .reuse
                .unwrap()
                .nickname_error
                == Some(NicknameError::RateLimited)
        );
        assert_eq!(w.pending(), 0);
        assert_eq!(
            w.register(&id, None).await.unwrap().status,
            LoginStatus::Authenticated
        );
        assert_eq!(w.count(REGISTER), 2);
    }
}
#[tokio::test]
async fn contradictory_or_unknown_registration_replies_keep_the_durable_fence() {
    for jsk in [false, true] {
        for response in [
            json!({"code":200,"data":{"success":false}}),
            json!({"code":200,"data":false}),
            json!({"code":500}),
            json!({"data":{}}),
        ] {
            let w = World::new(jsk, [info(), hint(), response.into()]).await;
            let id = w.prepare().await;
            let view = w.register(&id, None).await.unwrap();
            assert!(view.reuse.unwrap().phase == Phase::RecoveryRequired);
            assert_eq!(w.pending(), 1);
            assert!(w.register(&id, None).await.is_err());
            assert_eq!(w.count(REGISTER), 1);
        }
    }
}
#[tokio::test]
async fn registration_challenges_keep_the_durable_fence_and_never_offer_original_login_captcha_replay()
 {
    for code in [8821, 8830, 8860, 8861, 702, 8840, 8820, 304, 8831] {
        let w = World::new(true, [info(), hint(), json!({"code":code}).into()]).await;
        let id = w.prepare().await;
        let v = w.register(&id, None).await.unwrap();
        assert_eq!(v.status, LoginStatus::Error);
        assert!(!v.captcha_available);
        assert!(v.reuse.unwrap().phase == Phase::RecoveryRequired);
        assert_eq!(w.pending(), 1);
        assert_eq!(w.count(REGISTER), 1);
    }
}
#[tokio::test(start_paused = true)]
async fn held_registration_is_cancelled_bounded_or_aborted_without_losing_the_durable_unknown_intent()
 {
    for action in ["cancel", "timeout", "abort"] {
        let (pending, mut sender) = gate();
        let w = World::new(true, [info(), hint(), pending]).await;
        let id = w.prepare().await;
        let client = w.client.clone();
        let op = w.id.clone();
        let before = w.http.calls().len();
        let task = tokio::spawn(async move {
            client
                .login_command(
                    &op,
                    Cmd::ReuseRegister {
                        verification_id: id,
                        nickname: None,
                    },
                )
                .await
        });
        w.http.wait(before + 1).await;
        assert_eq!(w.pending(), 1);
        match action {
            "cancel" => {
                w.client.cancel_login(&w.id).unwrap();
                tokio::time::timeout(Duration::from_secs(1), sender.closed())
                    .await
                    .unwrap();
                assert!(task.await.unwrap().is_err());
            }
            "timeout" => {
                tokio::time::advance(Duration::from_secs(46)).await;
                assert!(task.await.unwrap().is_err());
                assert!(w.phase() == Phase::RecoveryRequired);
            }
            _ => {
                task.abort();
                assert!(task.await.err().unwrap().is_cancelled());
                assert!(w.phase() == Phase::RecoveryRequired);
            }
        }
        assert_eq!(w.pending(), 1);
        assert_eq!(w.count(REGISTER), 1);
        assert_ne!(
            w.client.account().unwrap().status,
            SessionStatus::Authenticated
        );
    }
}
#[tokio::test]
async fn interrupted_registration_receipt_is_only_reconciled_readonly_and_never_resubmitted() {
    let HttpResponse::Ready(Ok(mut partial)) = authorized() else {
        unreachable!()
    };
    partial.body_status = BodyStatus::Interrupted;
    let w = World::new(
        false,
        [info(), hint(), HttpResponse::Ready(Ok(partial)), identity()],
    )
    .await;
    let id = w.prepare().await;
    assert!(w.register(&id, None).await.is_err());
    assert_eq!(w.pending(), 1);
    assert_eq!(
        w.run(Cmd::Reconcile).await.unwrap().status,
        LoginStatus::Authenticated
    );
    assert_eq!(w.count(REGISTER), 1);
    assert_eq!(w.pending(), 0);
    let selected = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc)
        .unwrap()
        .current(Default::default())
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
