use super::support::{
    ScriptApi,
    http::{HttpResponse, ScriptHttp, gate, reply},
};
use cumulus_core::{
    Error,
    account::{
        AccountSession, SessionToken,
        credentials::Credentials,
        resources::{AccountResources, MutationPhase, MutationRecord, ResourceKind},
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{
            ApiOrigin, ClientProfile, MusicHttpClient, OFFICIAL_WINDOWS_USER_AGENT, RequestOptions,
        },
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{Notify, oneshot},
    task::JoinHandle,
};

const LIKE: &str = "/api/song/like";
const LIKED: &str = "/api/song/like/get";
const ACCOUNT: &str = "/api/w/nuser/account/get";

#[derive(Default)]
struct TokenGate {
    entered: Notify,
    pending: Mutex<Option<oneshot::Receiver<String>>>,
}

impl TokenGate {
    fn hold(&self) -> oneshot::Sender<String> {
        let (tx, rx) = oneshot::channel();
        assert!(self.pending.lock().unwrap().replace(rx).is_none());
        tx
    }

    async fn wait(&self) {
        tokio::time::timeout(Duration::from_secs(3), self.entered.notified())
            .await
            .expect("real request context never reached the token provider");
    }
}

impl TokenProvider for TokenGate {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            assert_eq!(business, "fixture-like");
            let pending = self.pending.lock().unwrap().take();
            self.entered.notify_one();
            Ok(match pending {
                Some(pending) => pending.await.expect("fixture token released"),
                None => "synthetic-check-token".into(),
            })
        })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    store: Arc<StateStore>,
    session: Arc<AccountSession>,
    resources: Arc<AccountResources>,
    token: SessionToken,
    context: Arc<TokenGate>,
    wire: Arc<ScriptHttp>,
}

impl Fixture {
    async fn new(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self::with_cookies(&["MUSIC_U=synthetic-like-fixture; Path=/; Secure"], replies).await
    }

    async fn with_cookies(
        cookies: &[&str],
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let origin = ApiOrigin::Pc;
        let credentials = Arc::new(Credentials::open(store.clone(), origin).unwrap());
        let target = origin.url();
        credentials
            .record_response(
                &target,
                &cookies.iter().map(ToString::to_string).collect::<Vec<_>>(),
                None,
                &credentials.request_credentials(&target).unwrap().revision,
            )
            .unwrap();
        let context = Arc::new(TokenGate::default());
        let wire = Arc::new(ScriptHttp::new(
            [json!({"code":200,"profile":{"userId":1}}).into()]
                .into_iter()
                .chain(replies),
        ));
        let http = Arc::new(MusicHttpClient::with_backend(
            credentials,
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some("synthetic-client-sign".into()),
            },
            RequestOptions::default(),
            Some(Arc::new(RequestContext::new(
                json!({"yidun#paths":{"whiteList":[{"businessId":"fixture-like","list":[LIKE]}]}}),
                origin.as_str().into(), context.clone(),
            ))),
            wire.clone(),
        ).unwrap());
        let session = Arc::new(AccountSession::new(http, Some(store.clone())));
        session.refresh().await.unwrap();
        let token = session.snapshot().unwrap().token;
        let resources = Arc::new(AccountResources::new(session.clone()).unwrap());
        Self {
            _root: root,
            store,
            session,
            resources,
            token,
            context,
            wire,
        }
    }

    fn start(&self, allowed: Arc<AtomicBool>) -> JoinHandle<cumulus_core::Result<MutationRecord>> {
        let resources = self.resources.clone();
        let token = self.token.clone();
        tokio::spawn(async move {
            resources
                .set_song_like_guarded_for(&token, &json!(20), true, &|| {
                    if allowed.load(Ordering::Acquire) {
                        Ok(())
                    } else {
                        Err(Error::StaleResponse)
                    }
                })
                .await
        })
    }

    fn calls(&self, paths: &[&str]) {
        assert_eq!(
            self.wire
                .calls()
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            paths
        );
    }

    fn journal(&self) -> Option<Value> {
        self.store
            .get("1", "resource-mutations", "liked:20")
            .unwrap()
    }

    fn unsent(&self) {
        let record = self.journal().unwrap();
        assert_eq!(record["phase"], "rejected");
        assert_eq!(record["error_class"], "not_sent");
        assert!(record.get("service_code").is_none());
        assert!(record.get("checked_at").is_none());
    }
}

fn likes(ids: &[u64]) -> HttpResponse {
    json!({"code":200,"ids":ids}).into()
}

#[tokio::test]
async fn held_membership_read_rejects_a_stale_action_even_if_it_became_a_noop() {
    let (read, release) = gate();
    let f = Fixture::new([read]).await;
    let allowed = Arc::new(AtomicBool::new(true));
    let change = f.start(allowed.clone());
    f.wire.wait(2).await;
    assert!(matches!(
        f.resources
            .set_membership_for(&f.token, ResourceKind::Liked, &json!(20), true)
            .await,
        Err(Error::MutationBusy)
    ));
    allowed.store(false, Ordering::Release);
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"ids":[20]}))))
            .is_ok()
    );
    assert!(matches!(change.await.unwrap(), Err(Error::StaleResponse)));
    f.calls(&[ACCOUNT, LIKED]);
    assert!(f.journal().is_none());
    assert_eq!(
        f.resources
            .membership_for(&f.token, ResourceKind::Liked, "20")
            .unwrap()
            .0,
        None
    );
}

#[tokio::test]
async fn stale_after_request_preparation_never_reaches_real_http_backend() {
    let f = Fixture::new([likes(&[10])]).await;
    let release = f.context.hold();
    let allowed = Arc::new(AtomicBool::new(true));
    let change = f.start(allowed.clone());
    f.context.wait().await;
    assert_eq!(f.journal().unwrap()["phase"], "sending");
    allowed.store(false, Ordering::Release);
    release.send("synthetic-check-token".into()).unwrap();
    assert!(matches!(change.await.unwrap(), Err(Error::StaleResponse)));
    f.calls(&[ACCOUNT, LIKED]);
    f.unsent();
}

#[tokio::test]
async fn account_handoff_while_preparing_keeps_the_known_unsent_record_in_the_original_owner() {
    let f = Fixture::new([likes(&[])]).await;
    let release = f.context.hold();
    let change = f.start(Arc::new(AtomicBool::new(true)));
    f.context.wait().await;
    f.session
        .attach(Arc::new(ScriptApi::new([
            json!({"code":200,"profile":{"userId":2}}).into(),
        ])))
        .unwrap();
    f.session.refresh().await.unwrap();
    release.send("synthetic-check-token".into()).unwrap();
    assert!(matches!(change.await.unwrap(), Err(Error::StaleResponse)));
    f.calls(&[ACCOUNT, LIKED]);
    f.unsent();
    assert!(f.resources.operations().unwrap().is_empty());
}

#[tokio::test]
async fn dropping_a_held_pre_dispatch_future_does_not_block_a_new_explicit_action() {
    let f = Fixture::new([
        likes(&[]),
        likes(&[]),
        json!({"code":502}).into(),
        likes(&[20]),
    ])
    .await;
    let release = f.context.hold();
    let change = f.start(Arc::new(AtomicBool::new(true)));
    f.context.wait().await;
    change.abort();
    assert!(change.await.is_err_and(|error| error.is_cancelled()));
    assert!(release.send("synthetic-check-token".into()).is_err());
    f.calls(&[ACCOUNT, LIKED]);
    f.unsent();

    let confirmed = f
        .start(Arc::new(AtomicBool::new(true)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(confirmed.phase, MutationPhase::Confirmed);
    assert_eq!(confirmed.service_code, Some(json!(502)));
    f.calls(&[ACCOUNT, LIKED, LIKED, LIKE, LIKED]);
    let sent = &f.wire.calls()[3];
    assert_eq!(sent.payload["trackId"], "20");
    assert_eq!(sent.payload["userid"], "1");
    assert_eq!(sent.payload["like"], "true");
    assert_eq!(sent.payload["checkToken"], "synthetic-check-token");
    let header: Value = serde_json::from_str(sent.payload["header"].as_str().unwrap()).unwrap();
    assert_eq!(header["clientSign"], "synthetic-client-sign");
    assert_eq!(header["X-antiCheatToken"], "synthetic-check-token");
    assert!(sent.payload.get("cache_key").is_none());
    assert!(sent.payload.get("rqRefer").is_none());
    assert!(sent.payload.get("songRqRefer").is_none());
    assert!(
        sent.cookie
            .as_deref()
            .unwrap()
            .contains("MUSIC_U=synthetic-like-fixture")
    );
}

#[tokio::test]
async fn stale_scope_after_dispatch_cannot_erase_lost_reply_or_trigger_replay() {
    let (write, release) = gate();
    let f = Fixture::new([likes(&[]), write, likes(&[20])]).await;
    let allowed = Arc::new(AtomicBool::new(true));
    let change = f.start(allowed.clone());
    f.wire.wait(3).await;
    allowed.store(false, Ordering::Release);
    assert!(
        release
            .send(Err(Error::Transport(
                "synthetic lost acknowledgment".into()
            )))
            .is_ok()
    );
    assert!(matches!(change.await.unwrap(), Err(Error::Transport(_))));
    assert_eq!(f.journal().unwrap()["phase"], "uncertain");
    assert!(matches!(
        f.start(Arc::new(AtomicBool::new(true))).await.unwrap(),
        Err(Error::MutationUncertain)
    ));
    f.calls(&[ACCOUNT, LIKED, LIKE]);
    assert_eq!(
        f.resources
            .reconcile_expected(&f.token, ResourceKind::Liked, "20")
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    f.calls(&[ACCOUNT, LIKED, LIKE, LIKED]);
}

#[tokio::test]
async fn same_revision_different_account_and_song_like_scopes_never_preflight_or_write() {
    let f = Fixture::with_cookies(
        &[
            "MUSIC_U=verified-account; Path=/eapi/w/nuser/account; Secure",
            "MUSIC_U=different-like-account; Path=/eapi/song/like; Secure",
        ],
        [],
    )
    .await;
    assert!(matches!(
        f.start(Arc::new(AtomicBool::new(true))).await.unwrap(),
        Err(Error::CredentialChanged)
    ));
    f.calls(&[ACCOUNT]);
    assert!(f.journal().is_none());
}

#[tokio::test]
async fn acknowledged_write_that_rotates_credentials_leaves_read_only_recovery() {
    let mut response = reply(json!({"code":200}));
    response.set_cookies = vec!["MUSIC_U=after-write; Path=/; Secure".into()];
    let f = Fixture::new([likes(&[]), HttpResponse::Ready(Ok(response)), likes(&[20])]).await;
    assert!(matches!(
        f.start(Arc::new(AtomicBool::new(true))).await.unwrap(),
        Err(Error::CredentialChanged)
    ));
    assert_eq!(f.journal().unwrap()["phase"], "verifying");
    f.calls(&[ACCOUNT, LIKED, LIKE]);
    assert_eq!(
        f.resources
            .reconcile_expected(&f.token, ResourceKind::Liked, "20")
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    f.calls(&[ACCOUNT, LIKED, LIKE, LIKED]);
}
