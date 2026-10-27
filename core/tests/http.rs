use cumulus_core::{
    Error,
    account::credentials::Credentials,
    protocol::{
        api::{Api, ApiFuture},
        context::{RequestContext, TokenProvider},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, MusicHttpClient,
            RequestOptions, RequestPlan, RoutingPolicy, WireRoute,
        },
        wire::{crypto, eapi, serialization},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Notify, oneshot};

enum Reply {
    Ready(HttpReply),
    Pending(oneshot::Receiver<HttpReply>),
}
struct Backend {
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<RequestPlan>>,
    started: Notify,
}
impl Backend {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            calls: Mutex::new(vec![]),
            started: Notify::new(),
        }
    }
    async fn wait(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while self.calls.lock().unwrap().is_empty() {
                self.started.notified().await
            }
        })
        .await
        .unwrap();
    }
}
impl HttpBackend for Backend {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(plan);
            self.started.notify_one();
            let next = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected retry/request");
            Ok(match next {
                Reply::Ready(reply) => reply,
                Reply::Pending(receiver) => receiver.await.unwrap(),
            })
        })
    }
}
fn reply(value: Value) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::to_vec(&value).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: vec![],
        refresh_token: None,
        metadata: BTreeMap::new(),
    }
}
fn credentials() -> (tempfile::TempDir, Arc<Credentials>) {
    let tmp = tempfile::tempdir().unwrap();
    let state = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Arc::new(Credentials::open(state, ApiOrigin::Pc).unwrap());
    let target = ApiOrigin::Pc.url();
    let snapshot = credentials.request_credentials(&target).unwrap();
    credentials
        .record_response(
            &target,
            &[
                "MUSIC_U=synthetic-own-session; Path=/; Secure; HttpOnly".into(),
                "__csrf=synthetic-csrf; Path=/; Secure".into(),
            ],
            None,
            &snapshot.revision,
        )
        .unwrap();
    (tmp, credentials)
}
fn client(
    credentials: Arc<Credentials>,
    backend: Arc<Backend>,
    options: RequestOptions,
    context: Option<Arc<RequestContext>>,
) -> MusicHttpClient {
    MusicHttpClient::with_backend(
        credentials,
        ClientProfile {
            client_sign: Some("synthetic-device".into()),
            ..Default::default()
        },
        options,
        context,
        backend,
    )
    .unwrap()
}
fn body(plan: &RequestPlan) -> Value {
    let fields: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
        .into_owned()
        .collect();
    eapi::unwrap(&fields["params"]).unwrap().1
}
struct Tokens(AtomicUsize);
impl TokenProvider for Tokens {
    fn token<'a>(&'a self, _business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            Ok(format!(
                "synthetic-sdk-{}",
                self.0.fetch_add(1, Ordering::SeqCst)
            ))
        })
    }
}

#[tokio::test]
async fn frontend_auth_header_and_fresh_sdk_token_keep_their_separate_scopes() {
    let (_tmp, credentials) = credentials();
    let backend = Arc::new(Backend::new([Reply::Ready(reply(json!({"code":200})))]));
    let tokens = Arc::new(Tokens(AtomicUsize::new(0)));
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"b","list":["/api/read"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        tokens,
    ));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions::default(),
        Some(context),
    );
    client
        .call("/api/read", json!({"id":1}), true, &[200])
        .await
        .unwrap();
    let calls = backend.calls.lock().unwrap();
    let plan = &calls[0];
    let payload = body(plan);
    let header: Value = serde_json::from_str(payload["header"].as_str().unwrap()).unwrap();
    assert_eq!(payload["id"], "1");
    assert_eq!(header["requestId"], 0);
    assert!(header.get("MUSIC_U").is_none());
    assert!(header.get("__csrf").is_none());
    assert!(header.get("channel").is_none());
    assert_eq!(header["clientSign"], "synthetic-device");
    assert_eq!(payload["checkToken"], "synthetic-sdk-0");
    assert_eq!(plan.headers["X-antiCheatToken"], "synthetic-sdk-0");
    assert!(plan.headers["Cookie"].contains("MUSIC_U=synthetic-own-session"));
    assert!(!plan.headers["Cookie"].contains("requestId="));
    assert!(!plan.headers["Cookie"].contains("synthetic-sdk"));
    let mut canonical =
        serialization::official_data(&json!({"id":1,"checkToken":"synthetic-sdk-0"})).unwrap();
    canonical["e_r"] = json!(false);
    let (_, cache_key) = eapi::cache_envelope(&canonical).unwrap();
    assert_eq!(payload["cache_key"], cache_key);
    assert_eq!(
        plan.url
            .query_pairs()
            .find(|(name, _)| name == "cache_key")
            .unwrap()
            .1,
        cache_key
    );
    assert!(payload.get("e_r").is_none());
}

#[tokio::test]
async fn explicit_response_encryption_flag_overrides_transport_default_in_both_directions() {
    let (_tmp, credentials) = credentials();
    let mut encrypted = reply(json!({"code":8821}));
    encrypted.body = crypto::encrypt_ecb(&encrypted.body, eapi::EAPI_KEY).unwrap();
    let backend = Arc::new(Backend::new([
        Reply::Ready(encrypted),
        Reply::Ready(reply(json!({"code":200}))),
    ]));
    let login = client(
        credentials.clone(),
        backend.clone(),
        RequestOptions::default(),
        None,
    );
    assert_eq!(
        login
            .request("/api/w/login", json!({"e_r":true}), false)
            .await
            .unwrap()["code"],
        8821
    );
    let artist = client(
        credentials,
        backend.clone(),
        RequestOptions {
            encrypt_response: true,
            ..Default::default()
        },
        None,
    );
    artist
        .request("/api/artist/v3/detail", json!({"e_r":false}), false)
        .await
        .unwrap();
    let calls = backend.calls.lock().unwrap();
    assert!(calls[0].decrypt_response);
    assert_eq!(body(&calls[0])["e_r"], true);
    assert!(!calls[1].decrypt_response);
    assert!(body(&calls[1]).get("e_r").is_none());
}

#[tokio::test]
async fn native_cloud_metadata_preserves_numeric_fields_and_object_header() {
    let (_tmp, credentials) = credentials();
    let backend = Arc::new(Backend::new([Reply::Ready(reply(json!({"code":200})))]));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions::default(),
        None,
    );
    client
        .call(
            "/api/upload/cloud/info/v2",
            json!({"songid":"opaque-ticket","bitrate":320000}),
            false,
            &[200],
        )
        .await
        .unwrap();
    let calls = backend.calls.lock().unwrap();
    let payload = body(&calls[0]);
    assert_eq!(payload["bitrate"], 320000);
    assert!(payload["header"].is_object());
    assert_eq!(payload["header"]["MUSIC_U"], "synthetic-own-session");
    assert!(payload["header"]["requestId"].is_string());
    assert_eq!(payload["e_r"], false);
}

#[tokio::test]
async fn rejected_pc_write_uses_one_preselected_weapi_request_and_no_sdk_attempt() {
    let (_tmp, credentials) = credentials();
    let backend = Arc::new(Backend::new([Reply::Ready(reply(json!({"code":403})))]));
    let tokens = Arc::new(Tokens(AtomicUsize::new(0)));
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"b","list":["/api/playlist/"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        tokens.clone(),
    ));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions::default(),
        Some(context),
    );
    assert!(
        client
            .call(
                "/api/playlist/create",
                json!({"uid":"1","name":"synthetic","privacy":10}),
                false,
                &[200]
            )
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    let calls = backend.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].route, WireRoute::Weapi);
    assert_eq!(
        calls[0].url.as_str(),
        "https://interfacepc.music.163.com/weapi/playlist/create"
    );
    assert_eq!(tokens.0.load(Ordering::SeqCst), 0);
    assert!(!calls[0].headers.contains_key("X-antiCheatToken"));
    let fields: BTreeMap<_, _> = url::form_urlencoded::parse(&calls[0].body)
        .into_owned()
        .collect();
    assert_eq!(fields["encSecKey"].len(), 256);
}

#[tokio::test]
async fn explicit_official_eapi_policy_injects_token_and_never_falls_back_after_rejection() {
    let (_tmp, credentials) = credentials();
    let backend = Arc::new(Backend::new([Reply::Ready(reply(json!({"code":403})))]));
    let tokens = Arc::new(Tokens(AtomicUsize::new(0)));
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"b","list":["/api/playlist/create"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        tokens.clone(),
    ));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions {
            routing: RoutingPolicy::OfficialEapi,
            ..Default::default()
        },
        Some(context),
    );
    assert!(
        client
            .call(
                "/api/playlist/create",
                json!({"uid":"1","name":"isolated","privacy":10}),
                false,
                &[200]
            )
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    let calls = backend.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].route, WireRoute::Eapi);
    assert_eq!(
        calls[0].url.as_str(),
        "https://interfacepc.music.163.com/eapi/playlist/create"
    );
    assert_eq!(tokens.0.load(Ordering::SeqCst), 1);
    let value = body(&calls[0]);
    assert_eq!(
        value["checkToken"].as_str(),
        calls[0].headers.get("X-antiCheatToken").map(String::as_str)
    );
    assert!(calls[0].headers["Cookie"].contains("MUSIC_U=synthetic-own-session"));
}

struct PcPolicy;
impl cumulus_core::protocol::mconfig::ConfigBackend for PcPolicy {
    fn get<'a>(
        &'a self,
        _: &'a cumulus_core::protocol::mconfig::ConfigUrl,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Ok(
                json!({"yidun#paths":{"whiteList":[{"businessId":"b","list":["/api/playlist/create"]}]}}),
            )
        })
    }
}
#[tokio::test]
async fn official_pc_write_waits_for_configuration_without_creating_an_unknown_remote_write() {
    use cumulus_core::protocol::mconfig::{APPLICATION_KEY, PcConfig};
    let (tmp, credentials) = credentials();
    let pc = PcConfig::new(
        Arc::new(StateStore::open(tmp.path().join("config")).unwrap()),
        Arc::new(PcPolicy),
    )
    .unwrap();
    let tokens = Arc::new(Tokens(AtomicUsize::new(0)));
    let context =
        Arc::new(RequestContext::from_pc(pc.clone(), "3.1.42.205594", tokens.clone()).unwrap());
    let backend = Arc::new(Backend::new([Reply::Ready(reply(
        json!({"code":200,"id":"7"}),
    ))]));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions {
            routing: RoutingPolicy::OfficialPcPlaylists,
            ..Default::default()
        },
        Some(context),
    );
    let data = json!({"uid":"1","name":"isolated","privacy":10});
    let error = client
        .call("/api/playlist/create", data.clone(), false, &[200])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::RequestNotSent(_)));
    let public = cumulus_core::client::ClientError::from(error);
    assert_eq!(public.code, "request_not_sent");
    assert!(public.retryable);
    assert!(backend.calls.lock().unwrap().is_empty());
    assert_eq!(tokens.0.load(Ordering::SeqCst), 0);
    pc.resolve(&BTreeMap::from([("MConfig-Info".into(),json!({APPLICATION_KEY:{"version":1,"os":"pc","fullUrl":"https://d5.music.126.net/obj/fixture/config.data"}}).to_string())])).await.unwrap();
    assert_eq!(
        client
            .call("/api/playlist/create", data, false, &[200])
            .await
            .unwrap()["id"],
        "7"
    );
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
    assert_eq!(tokens.0.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn official_pc_batch_does_not_change_wire_after_token_injection_adds_a_field() {
    let (_tmp, credentials) = credentials();
    let backend = Arc::new(Backend::new([Reply::Ready(reply(json!({"code":200})))]));
    let context = Arc::new(RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"b","list":["/api/batch"]}]}}),
        ApiOrigin::Pc.as_str().into(),
        Arc::new(Tokens(AtomicUsize::new(0))),
    ));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions {
            routing: RoutingPolicy::OfficialPcPlaylists,
            ..Default::default()
        },
        Some(context),
    );
    let fields = json!({"/api/playlist/update/name":"{}","/api/playlist/tags/update":"{}","/api/playlist/desc/update":"{}"});
    client
        .call("/api/batch", fields, false, &[200])
        .await
        .unwrap();
    let calls = backend.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].route, WireRoute::Eapi);
    assert!(body(&calls[0])["checkToken"].is_string());
}

#[tokio::test]
async fn interrupted_login_body_preserves_cookie_for_read_only_reconciliation() {
    let (_tmp, credentials) = credentials();
    credentials.clear().unwrap();
    let mut partial = reply(json!({"code":200}));
    partial.body_status = BodyStatus::Interrupted;
    partial.set_cookies = vec!["MUSIC_U=synthetic-new-session; Path=/; Secure; HttpOnly".into()];
    partial.refresh_token = Some("unacknowledged-refresh".into());
    let backend = Arc::new(Backend::new([
        Reply::Ready(partial),
        Reply::Ready(reply(json!({"code":200,"profile":{"userId":101}}))),
    ]));
    let client = client(
        credentials.clone(),
        backend.clone(),
        RequestOptions::default(),
        None,
    );
    assert!(matches!(
        client
            .request("/api/w/login", json!({"username":"synthetic"}), false)
            .await,
        Err(Error::Transport(_))
    ));
    let recovered = credentials.login_credentials().unwrap();
    assert_eq!(recovered.token.as_deref(), Some("synthetic-new-session"));
    assert!(recovered.refresh_token.is_none());
    assert_eq!(client.account().await.unwrap()["userId"], 101);
    let calls = backend.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].url.path().ends_with("/w/nuser/account/get"));
    assert!(calls[1].headers["Cookie"].contains("MUSIC_U=synthetic-new-session"));
}

#[tokio::test]
async fn refresh_header_only_commits_for_acknowledged_login_or_refresh_calls() {
    let (_tmp, credentials) = credentials();
    let replies = [
        ("/api/read", 200, "ignored"),
        ("/api/login/qrcode/client/login", 802, "pending"),
        ("/api/login/qrcode/client/login", 803, "committed"),
    ];
    let backend = Arc::new(Backend::new(replies.iter().map(|(_, code, token)| {
        let mut value = reply(json!({"code":code}));
        value.refresh_token = Some((*token).into());
        Reply::Ready(value)
    })));
    let client = client(
        credentials.clone(),
        backend,
        RequestOptions::default(),
        None,
    );
    for (index, (path, _, _)) in replies.iter().enumerate() {
        client.request(path, json!({}), false).await.unwrap();
        assert_eq!(
            credentials
                .login_credentials()
                .unwrap()
                .refresh_token
                .as_deref(),
            if index == 2 { Some("committed") } else { None }
        );
    }
}

#[tokio::test]
async fn encrypted_response_and_http_error_codes_are_kept_distinct() {
    let (_tmp, credentials) = credentials();
    let mut encrypted = reply(json!({"code":200}));
    encrypted.body = crypto::encrypt_ecb(&encrypted.body, eapi::EAPI_KEY).unwrap();
    let mut expired = reply(json!({"code":301}));
    expired.status = 401;
    let backend = Arc::new(Backend::new([
        Reply::Ready(encrypted),
        Reply::Ready(expired),
    ]));
    let client = client(
        credentials,
        backend.clone(),
        RequestOptions {
            encrypt_response: true,
            ..Default::default()
        },
        None,
    );
    assert_eq!(
        client
            .call("/api/test", json!({}), false, &[200])
            .await
            .unwrap(),
        json!({"code":200})
    );
    assert!(client.response_info().unwrap().unwrap().decrypted);
    let error = client
        .call("/api/test", json!({}), false, &[200])
        .await
        .unwrap_err();
    assert!(error.is_service_code(301));
    assert!(matches!(error, Error::HttpStatus { status: 401, .. }));
    assert_eq!(backend.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn logout_during_http_request_prevents_late_session_resurrection() {
    let (_tmp, credentials) = credentials();
    let (release, receiver) = oneshot::channel();
    let backend = Arc::new(Backend::new([Reply::Pending(receiver)]));
    let client = Arc::new(client(
        credentials.clone(),
        backend.clone(),
        RequestOptions::default(),
        None,
    ));
    let request = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .request("/api/login/qrcode/client/login", json!({}), false)
                .await
        })
    };
    backend.wait().await;
    credentials.clear().unwrap();
    let mut response = reply(json!({"code":803}));
    response.set_cookies = vec!["MUSIC_U=late-session; Path=/; Secure".into()];
    response.refresh_token = Some("late-refresh".into());
    assert!(release.send(response).is_ok());
    assert!(matches!(
        request.await.unwrap(),
        Err(Error::CredentialChanged)
    ));
    assert!(credentials.login_credentials().unwrap().token.is_none());
}

#[tokio::test]
async fn redirects_oversized_bodies_and_wrong_context_origin_do_not_claim_success() {
    let (_tmp, credentials) = credentials();
    let mut redirect = reply(json!({"code":200}));
    redirect.status = 302;
    redirect.set_cookies = vec!["injected=1; Path=/".into()];
    let mut large = reply(json!({"code":200}));
    large.body_status = BodyStatus::TooLarge;
    let backend = Arc::new(Backend::new([Reply::Ready(redirect), Reply::Ready(large)]));
    let client = client(
        credentials.clone(),
        backend.clone(),
        RequestOptions::default(),
        None,
    );
    assert!(matches!(
        client.request("/api/test", json!({}), false).await,
        Err(Error::Transport(_))
    ));
    assert!(
        !credentials
            .cookies_for(&ApiOrigin::Pc.url())
            .unwrap()
            .iter()
            .any(|(name, _)| name == "injected")
    );
    assert!(matches!(
        client.request("/api/test", json!({}), false).await,
        Err(Error::Response(_))
    ));
    let context = Arc::new(RequestContext::new(
        json!({}),
        ApiOrigin::Interface.as_str().into(),
        Arc::new(Tokens(AtomicUsize::new(0))),
    ));
    assert!(
        MusicHttpClient::with_backend(
            credentials,
            ClientProfile::default(),
            RequestOptions::default(),
            Some(context),
            backend
        )
        .is_err()
    );
}
