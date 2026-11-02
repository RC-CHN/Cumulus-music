mod support;
use cumulus_core::{
    Error,
    account::{SessionStatus, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{LoginCommand as Cmd, LoginMode as Mode},
    },
    protocol::{http::ApiOrigin, wire::eapi},
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::http::{HttpResponse, ScriptHttp, gate, reply};

fn frozen(name: &str, actual: &Value) {
    let vectors: Value =
        serde_json::from_str(include_str!("fixtures/jsk-login-official.json")).unwrap();
    let mut actual = actual.clone();
    let map = actual.as_object_mut().unwrap();
    map.remove("header");
    map.remove("e_r");
    let mut expected = vectors[name].clone();
    // M/48/M/402 form conversion stringifies primitive fields before EAPI.
    for value in expected.as_object_mut().unwrap().values_mut() {
        if value.is_number() {
            *value = value.to_string().into();
        }
    }
    if name == "phonePassword" {
        for key in ["once", "nonce"] {
            actual[key] = expected[key].clone();
        }
    }
    assert_eq!(actual, expected);
}

fn flags(jsk: bool, benefit: bool) -> HttpResponse {
    json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":if jsk {"t1"} else {"c"}},{"expName":"PH-PC-newLoginBenefit","expGroupName":if benefit {"t1"} else {"c"}}]}).into()
}
fn identity(uid: u64) -> HttpResponse {
    json!({"code":200,"profile":{"userId":uid,"nickname":format!("synthetic {uid}")}}).into()
}
fn authorized(code: u32, uid: u64, token: &str) -> HttpResponse {
    let mut r = reply(json!({"code":code}));
    r.set_cookies = vec![format!(
        "MUSIC_U=jsk-fixture-{uid}; Path=/; Secure; HttpOnly"
    )];
    r.refresh_token = Some(token.into());
    HttpResponse::Ready(Ok(r))
}
fn email() -> Cmd {
    Cmd::Email {
        address: "me@example.test".into(),
        password: "fixture-password".into(),
        remember: true,
    }
}
struct World {
    root: tempfile::TempDir,
    http: Arc<ScriptHttp>,
    client: Arc<MusicClient>,
}
impl World {
    fn new(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let http = Arc::new(ScriptHttp::new(replies));
        let client = Arc::new(
            MusicClient::open_with_http_backend(root.path(), None, http.clone())
                .unwrap()
                .with_official_login_policy(),
        );
        Self { root, http, client }
    }
    fn epoch(&self) -> String {
        self.client.account().unwrap().epoch
    }
    async fn begin(&self, mode: Mode) -> String {
        self.client
            .begin_login(&self.epoch(), mode, None)
            .await
            .unwrap()
            .operation_id
    }
    fn refresh(&self) -> Option<String> {
        ProfileRegistry::open(self.root.path(), ApiOrigin::Pc)
            .unwrap()
            .current(Default::default())
            .unwrap()
            .unwrap()
            .credentials
            .login_credentials()
            .unwrap()
            .refresh_token
    }
}
#[tokio::test]
async fn email_uses_jsk_endpoint_and_persists_server_refresh_header_before_identity_commit() {
    let w = World::new([
        flags(true, true),
        authorized(200, 2, "jsk-refresh"),
        identity(2),
    ]);
    let id = w.begin(Mode::Email).await;
    let view = w.client.login_command(&id, email()).await.unwrap();
    assert_eq!(w.refresh().as_deref(), Some("jsk-refresh"));
    let calls = w.http.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1].path, "/api/login");
    frozen("email", &calls[1].payload);
    assert_eq!(calls[1].payload["fromPage"], "JSK");
    assert_eq!(
        calls[1].payload["password"],
        eapi::md5_hex(b"fixture-password")
    );
    for absent in ["remember", "https", "type"] {
        assert!(calls[1].payload.get(absent).is_none());
    }
    assert_eq!(calls[1].login_chain.as_deref(), Some(""));
    assert!(calls[1].music_tu.is_none());
    assert_eq!(calls[2].path, "/api/w/nuser/account/get");
    assert!(calls[2].login_chain.is_none());
    let public = serde_json::to_string(&view).unwrap();
    for secret in ["jsk-refresh", "fixture-password", "jsk-fixture-2"] {
        assert!(!public.contains(secret));
    }
}
#[tokio::test]
async fn phone_password_uses_distinct_uuid4_once_nonce_and_keeps_remember_out_of_wire() {
    let w = World::new([
        flags(true, true),
        authorized(200, 2, "phone-refresh"),
        identity(2),
    ]);
    let id = w.begin(Mode::PhonePassword).await;
    w.client
        .login_command(
            &id,
            Cmd::PhonePassword {
                phone: "13800138000".into(),
                country_code: "86".into(),
                password: "fixture-password".into(),
                remember: false,
            },
        )
        .await
        .unwrap();
    let calls = w.http.calls();
    let call = &calls[1];
    assert_eq!(call.path, "/api/login/cellphone");
    frozen("phonePassword", &call.payload);
    assert_eq!(call.payload["countrycode"], "86");
    assert!(call.payload.get("countryCode").is_none());
    assert_eq!(call.payload["secureCaptcha"], "");
    assert_eq!(call.payload["version"], "1");
    assert!(call.payload.get("remember").is_none());
    for name in ["once", "nonce"] {
        let value = call.payload[name].as_str().unwrap();
        let id = uuid::Uuid::parse_str(value).unwrap();
        assert_eq!(id.get_version_num(), 4);
        assert_eq!(value.len(), 36);
    }
    assert_ne!(call.payload["once"], call.payload["nonce"]);
    let saved = w.client.saved_accounts(&w.epoch()).unwrap();
    assert!(!saved.value[0].auto_login);
    assert_eq!(w.refresh().as_deref(), Some("phone-refresh"));
}
#[tokio::test]
async fn pc_sms_keeps_explicit_send_and_omits_the_official_empty_urs_token() {
    let w = World::new([
        flags(true, true),
        json!({"code":200}).into(),
        authorized(200, 2, "sms-refresh"),
        identity(2),
    ]);
    let id = w.begin(Mode::PhoneSms).await;
    w.client
        .login_command(
            &id,
            Cmd::SendSms {
                phone: "13800138000".into(),
                country_code: "86".into(),
            },
        )
        .await
        .unwrap();
    w.client
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
    let calls = w.http.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[1].path, "/api/sms/captcha/sent");
    assert!(calls[1].login_chain.is_none());
    assert_eq!(calls[2].path, "/api/login/cellphone");
    frozen("phoneSms", &calls[2].payload);
    assert_eq!(calls[2].payload["countryCode"], "86");
    assert_eq!(calls[2].payload["captcha"], "123456");
    for absent in ["countrycode", "ursToken", "once", "nonce", "remember"] {
        assert!(calls[2].payload.get(absent).is_none());
    }
    assert_eq!(w.refresh().as_deref(), Some("sms-refresh"));
}
#[tokio::test]
async fn both_experiments_are_required_and_lookup_failure_never_turns_into_a_jsk_assignment() {
    for first in [
        flags(true, false),
        flags(false, true),
        flags(false, false),
        HttpResponse::Ready(Err(Error::Transport(
            "fixture configuration unavailable".into(),
        ))),
    ] {
        let w = World::new([first, json!({"code":502}).into()]);
        let id = w.begin(Mode::Email).await;
        w.client.login_command(&id, email()).await.unwrap();
        let calls = w.http.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].path, "/api/w/login");
        assert!(calls[1].login_chain.is_none());
        assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
    }
}
#[tokio::test]
async fn challenge_ticket_is_forwarded_only_inside_its_candidate_and_verification_refresh_is_saved()
{
    let mut challenge = reply(json!({"code":8830}));
    challenge.set_cookies =
        vec!["MUSIC_TU=fixture-ticket; Path=/verification-only; Secure; HttpOnly".into()];
    let w = World::new([
        flags(true, true),
        HttpResponse::Ready(Ok(challenge)),
        json!({"code":200}).into(),
        json!({"code":200,"data":{"types":["SECURITY_EMAIL"]}}).into(),
        json!({"code":200,"data":{"mailAddress":"m***@example.test"}}).into(),
        authorized(200, 3, "verified-refresh"),
        identity(3),
        identity(3),
        flags(true, true),
        json!({"code":502}).into(),
    ]);
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, email()).await.unwrap();
    w.client
        .login_command(&id, Cmd::VerificationMethods)
        .await
        .unwrap();
    let sent = w
        .client
        .login_command(&id, Cmd::SecurityMailSend)
        .await
        .unwrap();
    w.client
        .login_command(
            &id,
            Cmd::SecurityMailVerify {
                verification_id: sent.email.unwrap().id,
                code: "123456".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(w.refresh().as_deref(), Some("verified-refresh"));
    w.client.restore_account().await.unwrap();
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, email()).await.unwrap();
    let calls = w.http.calls();
    assert_eq!(calls[2].path, "/api/user/2fa/begin");
    assert_eq!(calls[3].path, "/api/user/2fa/list");
    assert!(
        !calls[2]
            .cookie
            .as_deref()
            .unwrap_or("")
            .contains("MUSIC_TU")
    );
    assert_eq!(calls.len(), 10);
    for call in &calls[2..=5] {
        assert_eq!(call.music_tu.as_deref(), Some("fixture-ticket"));
        assert_eq!(call.login_chain.as_deref(), Some(""));
    }
    for index in [1, 6, 7, 8, 9] {
        assert!(calls[index].music_tu.is_none());
    }
    assert!(
        !calls[8]
            .cookie
            .as_deref()
            .unwrap_or("")
            .contains("MUSIC_TU")
    );
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("synthetic 3")
    );
}
struct Captcha;
impl cumulus_core::client::LoginCaptchaProvider for Captcha {
    fn solve(&self) -> cumulus_core::protocol::api::ApiFuture<'_, String> {
        Box::pin(async { Ok("completed-fixture-captcha".into()) })
    }
}
#[tokio::test]
async fn explicit_captcha_replay_preserves_jsk_nonce_pair_and_never_changes_protocol() {
    let w = World::new([
        flags(true, true),
        json!({"code":8821}).into(),
        authorized(200, 2, "captcha-refresh"),
        identity(2),
    ]);
    let id = w.begin(Mode::PhonePassword).await;
    w.client
        .login_command(
            &id,
            Cmd::PhonePassword {
                phone: "13800138000".into(),
                country_code: "86".into(),
                password: "fixture-password".into(),
                remember: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(w.http.calls().len(), 2);
    w.client.solve_login_captcha(&id, &Captcha).await.unwrap();
    let calls = w.http.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[1].path, calls[2].path);
    for name in ["once", "nonce", "password"] {
        assert_eq!(calls[1].payload[name], calls[2].payload[name]);
    }
    assert_eq!(
        calls[2].payload["secureCaptcha"],
        "completed-fixture-captcha"
    );
    assert_eq!(w.refresh().as_deref(), Some("captcha-refresh"));
}
#[tokio::test]
async fn jsk_qr_uses_type_five_empty_captcha_and_one_second_poll_hint_then_adopts_identity() {
    let w = World::new([
        flags(true, true),
        json!({"code":200,"unikey":"fixture-qr-key"}).into(),
        json!({"code":802}).into(),
        authorized(803, 2, "qr-refresh"),
        identity(2),
    ]);
    let view = w.client.begin_qr().await.unwrap();
    assert_eq!(view.poll_after_ms, 1000);
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("fixture-qr-key")
    );
    w.client.poll_qr(&view.operation_id).await.unwrap();
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
    w.client.poll_qr(&view.operation_id).await.unwrap();
    assert_eq!(w.refresh().as_deref(), Some("qr-refresh"));
    let calls = w.http.calls();
    assert_eq!(calls.len(), 5);
    frozen("qrCreate", &calls[1].payload);
    frozen("qrPoll", &calls[2].payload);
    for call in &calls[1..=3] {
        assert_eq!(call.payload["type"], "5");
        assert_eq!(call.payload["fromPage"], "JSK");
        assert_eq!(call.login_chain.as_deref(), Some(""));
        assert!(call.payload.get("lastUniKey").is_none());
    }
    assert_eq!(calls[2].payload["secureCaptcha"], "");
    assert!(calls[4].login_chain.is_none());
}
#[tokio::test]
async fn cancelled_jsk_authorization_cannot_publish_its_cookie_or_identity() {
    let (pending, release) = gate();
    let w = World::new([
        flags(true, true),
        json!({"code":200,"unikey":"fixture-cancel-key"}).into(),
        pending,
    ]);
    let view = w.client.begin_qr().await.unwrap();
    let client = w.client.clone();
    let id = view.operation_id.clone();
    let task = tokio::spawn(async move { client.poll_qr(&id).await });
    w.http.wait(3).await;
    w.client.cancel_qr(&view.operation_id).unwrap();
    let HttpResponse::Ready(response) = authorized(803, 2, "cancelled-refresh") else {
        unreachable!()
    };
    assert!(release.send(response).is_ok());
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(w.client.account().unwrap().status, SessionStatus::Unknown);
    assert_eq!(w.http.calls().len(), 3);
    assert!(
        w.client
            .saved_accounts(&w.epoch())
            .unwrap()
            .value
            .is_empty()
    );
}

#[tokio::test]
async fn jsk_verification_begins_once_before_the_list_even_if_begin_reply_is_lost() {
    let mut challenge = reply(json!({"code":8830}));
    challenge.set_cookies = vec!["MUSIC_TU=verify-begin-ticket; Path=/; Secure".into()];
    let w = World::new([
        flags(true, true),
        HttpResponse::Ready(Ok(challenge)),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost begin reply".into()))),
        json!({"code":200,"data":{"types":["SECURITY_EMAIL"]}}).into(),
    ]);
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, email()).await.unwrap();
    let view = w
        .client
        .login_command(&id, Cmd::VerificationMethods)
        .await
        .unwrap();
    assert_eq!(view.challenge.methods, vec!["SECURITY_EMAIL"]);
    let calls = w.http.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[2].path, "/api/user/2fa/begin");
    assert_eq!(calls[3].path, "/api/user/2fa/list");
    assert_eq!(calls[3].music_tu.as_deref(), Some("verify-begin-ticket"));
}
#[tokio::test]
async fn a_captcha_cookie_is_not_promoted_to_a_jsk_verification_header() {
    let mut captcha = reply(json!({"code":8821}));
    captcha.set_cookies = vec!["MUSIC_TU=cookie-without-verify-event; Path=/; Secure".into()];
    let w = World::new([
        flags(true, true),
        HttpResponse::Ready(Ok(captcha)),
        authorized(200, 2, "captcha-only-refresh"),
        identity(2),
    ]);
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, email()).await.unwrap();
    w.client.solve_login_captcha(&id, &Captcha).await.unwrap();
    let calls = w.http.calls();
    assert!(calls[2].cookie.as_deref().unwrap().contains("MUSIC_TU"));
    assert!(calls[2].music_tu.is_none());
}

#[tokio::test]
async fn a_begin_acknowledgement_does_not_replace_the_sdk_verify_manager_ticket() {
    let mut challenge = reply(json!({"code":8830}));
    challenge.set_cookies = vec!["MUSIC_TU=original-verify-ticket; Path=/; Secure".into()];
    let mut begun = reply(json!({"code":8830}));
    begun.set_cookies = vec!["MUSIC_TU=begin-cookie-without-event; Path=/; Secure".into()];
    let w = World::new([
        flags(true, true),
        HttpResponse::Ready(Ok(challenge)),
        HttpResponse::Ready(Ok(begun)),
        json!({"code":200,"data":{"types":["SECURITY_EMAIL"]}}).into(),
    ]);
    let id = w.begin(Mode::Email).await;
    w.client.login_command(&id, email()).await.unwrap();
    w.client
        .login_command(&id, Cmd::VerificationMethods)
        .await
        .unwrap();
    let calls = w.http.calls();
    assert_eq!(calls[3].music_tu.as_deref(), Some("original-verify-ticket"));
    assert!(
        calls[3]
            .cookie
            .as_deref()
            .unwrap()
            .contains("begin-cookie-without-event")
    );
}
