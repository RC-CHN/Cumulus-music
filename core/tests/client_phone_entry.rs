mod support;
use cumulus_core::{
    Error, Result,
    account::{
        SessionStatus,
        login::{LoginStatus, NicknameError, PhoneEntryIssue as Issue, PhoneEntryPhase as Phase},
        profiles::ProfileRegistry,
    },
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode, LoginView},
    },
    protocol::http::{ApiOrigin, BodyStatus},
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use support::http::{HttpResponse, ScriptHttp, gate, reply};
const EXIST: &str = "/api/cellphone/existence/check";
const SEND: &str = "/api/sms/captcha/sent";
const MIDDLE: &str = "/api/middle/captcha/sent/v1";
const VERIFY: &str = "/api/sms/captcha/verify";
const HINT: &str = "/api/nickname/hint/get";
const DUP: &str = "/api/nickname/duplicated";
const REGISTER: &str = "/api/w/register/cellphone";
fn official_call(calls: &[support::http::WireCall], key: &str) {
    let frozen: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/phone-entry-official.json")).unwrap();
    let vector = &frozen["vectors"][key];
    let call = calls.iter().find(|c| c.path == vector["path"]).unwrap();
    // The transport adds its native header. Compare all recovered business fields
    // and reject extra business fields, rather than only checking selected values.
    let mut payload = call.payload.clone();
    payload.as_object_mut().unwrap().remove("header");
    assert_eq!(payload, vector["payload"], "official {key}");
}
fn assignment(middle: bool) -> HttpResponse {
    json!({"code":200,"data":[{"expName":"massagechange","expGroupName":if middle {"t1"}else{""}}]})
        .into()
}
fn existence(new_user: bool, skip: bool) -> HttpResponse {
    json!({"code":200,"exist":if new_user {-1}else{1},"canSkipSetNickname":skip}).into()
}
fn hint() -> HttpResponse {
    json!({"code":200,"hintNickname":"合成推荐昵称"}).into()
}
fn available() -> HttpResponse {
    json!({"code":200,"duplicated":false}).into()
}
fn identity() -> HttpResponse {
    json!({"code":200,"profile":{"userId":2,"nickname":"合成普通账号"}}).into()
}
fn authorized() -> HttpResponse {
    let mut r = reply(json!({"code":200}));
    r.set_cookies = vec!["MUSIC_U=synthetic-signup; Path=/; Secure; HttpOnly".into()];
    r.refresh_token = Some("synthetic-signup-refresh".into());
    HttpResponse::Ready(Ok(r))
}
struct World {
    root: tempfile::TempDir,
    client: Arc<MusicClient>,
    http: Arc<ScriptHttp>,
    id: String,
    entry: String,
    clock: Arc<Mutex<Instant>>,
}
impl World {
    async fn new(
        jsk: bool,
        middle: bool,
        new_user: bool,
        skip: bool,
        rest: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        let mut replies = Vec::new();
        if jsk {
            replies.push(json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]}).into());
        }
        replies.extend([
            assignment(middle),
            existence(new_user, skip),
            json!({"code":200}).into(),
        ]);
        replies.extend(rest);
        let root = tempfile::tempdir().unwrap();
        let http = Arc::new(ScriptHttp::new(replies));
        let clock = Arc::new(Mutex::new(Instant::now()));
        let timer = clock.clone();
        let client = MusicClient::open_with_http_backend(root.path(), None, http.clone())
            .unwrap()
            .with_login_resend_clock(Arc::new(move || *timer.lock().unwrap()));
        let client = Arc::new(if jsk {
            client.with_official_login_policy()
        } else {
            client
        });
        let id = start(&client).await;
        let v = client
            .login_command(
                &id,
                Cmd::PhoneStart {
                    phone: "13800000000".into(),
                    country_code: "86".into(),
                },
            )
            .await
            .unwrap();
        let entry = v.phone_entry.unwrap().id;
        Self {
            root,
            client,
            http,
            id,
            entry,
            clock,
        }
    }
    async fn run(&self, c: Cmd) -> Result<LoginView> {
        self.client.login_command(&self.id, c).await
    }
    async fn code(&self) -> LoginView {
        self.run(Cmd::PhoneCode {
            entry_id: self.entry.clone(),
            code: "246810".into(),
            remember: false,
        })
        .await
        .unwrap()
    }
    async fn register(&self, name: Option<&str>) -> Result<LoginView> {
        self.run(Cmd::PhoneRegister {
            entry_id: self.entry.clone(),
            nickname: name.map(str::to_owned),
        })
        .await
    }
    fn pending(&self) -> usize {
        ProfileRegistry::open(self.root.path(), ApiOrigin::Pc)
            .unwrap()
            .store()
            .values("transport", "phone-registration-v1")
            .unwrap()
            .len()
    }
}
async fn start(client: &MusicClient) -> String {
    client
        .begin_login(&client.account().unwrap().epoch, LoginMode::PhoneSms, None)
        .await
        .unwrap()
        .operation_id
}

#[tokio::test]
async fn both_protocols_use_legacy_preflight_and_named_signup_with_force_false() {
    for jsk in [false, true] {
        let w = World::new(
            jsk,
            false,
            true,
            true,
            [
                json!({"code":200}).into(),
                hint(),
                available(),
                authorized(),
                identity(),
            ],
        )
        .await;
        let v = w.code().await;
        assert_eq!(v.phone_entry.unwrap().phase, Phase::Nickname);
        assert_eq!(w.pending(), 0);
        assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
        assert_eq!(
            w.register(Some("社区昵称")).await.unwrap().status,
            LoginStatus::Authenticated
        );
        assert_eq!(w.pending(), 0);
        let calls = w.http.calls();
        for key in ["preflight", "sendPlain", "verify", "registerNamed"] {
            official_call(&calls, key);
        }
        for path in [EXIST, SEND, VERIFY, HINT, DUP, REGISTER] {
            let call = calls.iter().find(|c| c.path == path).unwrap();
            assert!(call.login_chain.is_none());
            assert!(call.payload.get("fromPage").is_none());
        }
        let send = calls.iter().find(|c| c.path == SEND).unwrap();
        assert_eq!(send.payload["scene"], "0");
        assert_eq!(send.payload["secrete"], "music_middleuser_pclogin");
        let register = calls.iter().find(|c| c.path == REGISTER).unwrap();
        assert_eq!(register.payload["force"], "false");
        assert_eq!(register.payload["nickname"], "社区昵称");
        assert!(register.payload.get("ursToken").is_none());
        let registry = ProfileRegistry::open(w.root.path(), ApiOrigin::Pc).unwrap();
        let selected = registry.current(Default::default()).unwrap().unwrap();
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
#[tokio::test]
async fn existing_user_skips_signup_verification_and_uses_the_selected_login_protocol() {
    for jsk in [false, true] {
        let w = World::new(jsk, false, false, false, [authorized(), identity()]).await;
        assert_eq!(w.code().await.status, LoginStatus::Authenticated);
        let calls = w.http.calls();
        let login = calls
            .iter()
            .find(|c| {
                c.path
                    == if jsk {
                        "/api/login/cellphone"
                    } else {
                        "/api/w/login/cellphone"
                    }
            })
            .unwrap();
        assert_eq!(login.payload["captcha"], "246810");
        assert_eq!(login.payload["phone"], "13800000000");
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c.path.as_str(), VERIFY | REGISTER | HINT))
        );
    }
}
#[tokio::test]
async fn middle_sms_group_is_native_only_and_resends_keep_the_immutable_binding_and_cooldown() {
    let w = World::new(false, true, false, false, [json!({"code":200}).into()]).await;
    assert!(w.http.calls().iter().any(|c| c.path == MIDDLE));
    official_call(&w.http.calls(), "sendMiddle");
    assert_eq!(
        w.run(Cmd::PhoneResend {
            entry_id: w.entry.clone()
        })
        .await
        .unwrap()
        .phone_entry
        .unwrap()
        .issue,
        Some(Issue::Cooldown)
    );
    assert_eq!(
        w.http.calls().iter().filter(|c| c.path == MIDDLE).count(),
        1
    );
    *w.clock.lock().unwrap() += Duration::from_secs(61);
    w.run(Cmd::PhoneResend {
        entry_id: w.entry.clone(),
    })
    .await
    .unwrap();
    let calls = w.http.calls();
    let sent: Vec<_> = calls.iter().filter(|c| c.path == MIDDLE).collect();
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter()
            .all(|c| c.payload["cellphone"] == "13800000000" && c.payload["ctcode"] == "86")
    );
    assert!(
        w.run(Cmd::SendSms {
            phone: "13900000000".into(),
            country_code: "86".into()
        })
        .await
        .is_err()
    );
    assert!(
        w.run(Cmd::PhoneResend {
            entry_id: "stale".into()
        })
        .await
        .is_err()
    );
}
#[tokio::test]
async fn nickname_permissions_format_and_duplicate_results_precede_any_registration_intent() {
    let w = World::new(
        false,
        false,
        true,
        false,
        [
            json!({"code":200}).into(),
            hint(),
            json!({"code":200,"duplicated":true}).into(),
        ],
    )
    .await;
    assert!(w.register(None).await.is_err());
    w.code().await;
    assert!(w.register(None).await.is_err());
    assert_eq!(w.pending(), 0);
    let before = w.http.calls().len();
    let v = w.register(Some("\u{85}abcd")).await.unwrap();
    assert_eq!(
        v.phone_entry.unwrap().nickname_error,
        Some(NicknameError::InvalidCharacters)
    );
    assert_eq!(w.http.calls().len(), before);
    let v = w.register(Some("已使用昵称")).await.unwrap();
    assert_eq!(
        v.phone_entry.unwrap().nickname_error,
        Some(NicknameError::Taken)
    );
    assert_eq!(w.pending(), 0);
    assert!(!w.http.calls().iter().any(|c| c.path == REGISTER));
}
#[tokio::test]
async fn default_nickname_is_only_offered_by_the_original_preflight_and_never_runs_on_cancel() {
    let w = World::new(
        false,
        false,
        true,
        true,
        [json!({"code":200}).into(), hint(), authorized(), identity()],
    )
    .await;
    w.code().await;
    assert_eq!(
        w.register(None).await.unwrap().status,
        LoginStatus::Authenticated
    );
    let calls = w.http.calls();
    let register = calls.iter().find(|c| c.path == REGISTER).unwrap();
    official_call(&calls, "registerDefault");
    assert_eq!(register.payload["isSkipSetNickname"], "true");
    assert!(register.payload.get("nickname").is_none());
    let w = World::new(
        false,
        false,
        true,
        true,
        [json!({"code":200}).into(), hint()],
    )
    .await;
    w.code().await;
    w.client.cancel_login(&w.id).unwrap();
    assert!(!w.http.calls().iter().any(|c| c.path == REGISTER));
    assert_eq!(w.pending(), 0);
}
#[tokio::test]
async fn invalid_code_does_not_open_nickname_or_cache_authorization() {
    let w = World::new(false, false, true, true, [json!({"code":503}).into()]).await;
    let v = w.code().await.phone_entry.unwrap();
    assert_eq!(v.phase, Phase::CodeRequired);
    assert_eq!(v.issue, Some(Issue::CodeInvalid));
    assert!(w.register(Some("社区昵称")).await.is_err());
    assert!(
        !w.http
            .calls()
            .iter()
            .any(|c| c.path == HINT || c.path == REGISTER)
    );
}
#[tokio::test]
async fn cancellation_aborts_a_held_preflight_without_sending_sms_or_late_publishing() {
    let (held, sender) = gate();
    let http = Arc::new(ScriptHttp::new([assignment(false), held]));
    let root = tempfile::tempdir().unwrap();
    let client =
        Arc::new(MusicClient::open_with_http_backend(root.path(), None, http.clone()).unwrap());
    let id = start(&client).await;
    let task = {
        let c = client.clone();
        let id = id.clone();
        tokio::spawn(async move {
            c.login_command(
                &id,
                Cmd::PhoneStart {
                    phone: "13800000000".into(),
                    country_code: "86".into(),
                },
            )
            .await
        })
    };
    http.wait(2).await;
    client.cancel_login(&id).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(
        sender
            .send(Ok(reply(json!({"code":200,"exist":-1}))))
            .is_err()
    );
    assert!(!http.calls().iter().any(|c| c.path == SEND));
    assert_eq!(client.account().unwrap().status, SessionStatus::Unknown);
}
#[tokio::test]
async fn lost_registration_without_cookie_is_fenced_across_restart_and_only_fresh_sms_login_settles_it()
 {
    let w = World::new(
        false,
        false,
        true,
        true,
        [
            json!({"code":200}).into(),
            hint(),
            HttpResponse::Ready(Err(Error::Transport("lost registration reply".into()))),
        ],
    )
    .await;
    w.code().await;
    assert!(w.register(None).await.is_err());
    let v = w.client.login_snapshot(&w.id).unwrap().phone_entry.unwrap();
    assert_eq!(v.phase, Phase::RecoveryRequired);
    assert!(!v.can_reconcile);
    assert_eq!(w.pending(), 1);
    assert!(w.register(None).await.is_err());
    w.client.cancel_login(&w.id).unwrap();
    let http = Arc::new(ScriptHttp::new([
        assignment(false),
        existence(true, true),
        json!({"code":200}).into(),
        authorized(),
        identity(),
    ]));
    let client = MusicClient::open_with_http_backend(w.root.path(), None, http.clone()).unwrap();
    let id = start(&client).await;
    let entry = client
        .login_command(
            &id,
            Cmd::PhoneStart {
                phone: "13800000000".into(),
                country_code: "86".into(),
            },
        )
        .await
        .unwrap()
        .phone_entry
        .unwrap();
    assert!(entry.registration_blocked);
    let v = client
        .login_command(
            &id,
            Cmd::PhoneCode {
                entry_id: entry.id,
                code: "246810".into(),
                remember: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(v.status, LoginStatus::Authenticated);
    assert_eq!(w.pending(), 0);
    assert!(
        !http
            .calls()
            .iter()
            .any(|c| c.path == REGISTER || c.path == VERIFY)
    );
}
#[tokio::test]
async fn partial_registration_cookie_can_only_be_reconciled_readonly_without_accepting_refresh_header()
 {
    let mut partial = reply(json!({"code":200}));
    partial.body_status = BodyStatus::Interrupted;
    partial.set_cookies = vec!["MUSIC_U=synthetic-signup; Path=/; Secure; HttpOnly".into()];
    partial.refresh_token = Some("must-not-be-accepted".into());
    let w = World::new(
        false,
        false,
        true,
        true,
        [
            json!({"code":200}).into(),
            hint(),
            HttpResponse::Ready(Ok(partial)),
            identity(),
        ],
    )
    .await;
    w.code().await;
    assert!(w.register(None).await.is_err());
    assert!(
        w.client
            .login_snapshot(&w.id)
            .unwrap()
            .phone_entry
            .unwrap()
            .can_reconcile
    );
    assert_eq!(
        w.run(Cmd::Reconcile).await.unwrap().status,
        LoginStatus::Authenticated
    );
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
    assert_eq!(
        w.http.calls().iter().filter(|c| c.path == REGISTER).count(),
        1
    );
}
#[tokio::test]
async fn request_not_sent_can_be_explicitly_retried_but_ambiguous_business_replies_cannot() {
    let w = World::new(
        false,
        false,
        true,
        true,
        [
            json!({"code":200}).into(),
            hint(),
            HttpResponse::Ready(Err(Error::RequestNotSent(Box::new(Error::State(
                "native preparation failed",
            ))))),
            authorized(),
            identity(),
        ],
    )
    .await;
    w.code().await;
    assert!(w.register(None).await.is_err());
    assert_eq!(w.pending(), 0);
    assert_eq!(
        w.client
            .login_snapshot(&w.id)
            .unwrap()
            .phone_entry
            .unwrap()
            .phase,
        Phase::Nickname
    );
    assert_eq!(
        w.register(None).await.unwrap().status,
        LoginStatus::Authenticated
    );
    for response in [
        json!({"code":200,"data":{"success":false}}),
        json!({"code":500}),
        json!({"data":{}}),
    ] {
        let w = World::new(
            false,
            false,
            true,
            true,
            [json!({"code":200}).into(), hint(), response.into()],
        )
        .await;
        w.code().await;
        assert_eq!(
            w.register(None).await.unwrap().phone_entry.unwrap().phase,
            Phase::RecoveryRequired
        );
        assert_eq!(w.pending(), 1);
        assert!(w.register(None).await.is_err());
    }
}

#[tokio::test]
async fn explicit_registration_rejections_clear_only_their_own_intent_and_expire_the_sms_when_needed()
 {
    for (code, error, phase) in [
        (250, NicknameError::AntiAbuse, Phase::Nickname),
        (509, NicknameError::RateLimited, Phase::Nickname),
        (503, NicknameError::SmsInvalid, Phase::CodeRequired),
        (504, NicknameError::PhoneRegistered, Phase::CodeRequired),
    ] {
        let w = World::new(
            false,
            false,
            true,
            true,
            [
                json!({"code":200}).into(),
                hint(),
                json!({"code":code}).into(),
                authorized(),
                identity(),
            ],
        )
        .await;
        w.code().await;
        let entry = w.register(None).await.unwrap().phone_entry.unwrap();
        assert_eq!(entry.phase, phase);
        assert_eq!(entry.nickname_error, Some(error));
        assert_eq!(w.pending(), 0);
        if code == 504 {
            assert!(!entry.new_user);
            assert_eq!(w.code().await.status, LoginStatus::Authenticated);
            assert_eq!(
                w.http.calls().iter().filter(|c| c.path == VERIFY).count(),
                1
            );
        } else if code == 503 {
            assert!(w.register(None).await.is_err());
            assert_eq!(
                w.http.calls().iter().filter(|c| c.path == REGISTER).count(),
                1
            );
        }
    }
}
