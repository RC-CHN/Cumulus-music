#[path = "support/http.rs"]
mod http_support;

use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        messages::{
            MessageCounts, MessageReadAllAction as Action, MessageReadAllState as Status,
            PcMessageConnectivity,
        },
    },
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions},
    storage::StateStore,
};
use http_support::{HttpResponse, ScriptHttp, gate, reply};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};

const COUNT: &str = "/api/pl/count";
const CLEAR: &str = "/api/msg/count/clear";
const JOURNAL: &str = "message-read-all-v1";

fn positive(private: u64) -> HttpResponse {
    json!({"code":200,"msg":private,"comment":0,"forward":0,"notice":1}).into()
}
fn zero() -> HttpResponse {
    json!({"code":200,"msg":0,"comment":0,"forward":0,"notice":0}).into()
}
fn rotation(credentials: &Credentials, cookies: &[&str]) {
    let url = ApiOrigin::Pc.url();
    let revision = credentials.request_credentials(&url).unwrap().revision;
    credentials
        .record_response(
            &url,
            &cookies.iter().map(ToString::to_string).collect::<Vec<_>>(),
            None,
            &revision,
        )
        .unwrap();
}

fn transport(
    path: &Path,
    replies: impl IntoIterator<Item = HttpResponse>,
) -> (Arc<MusicHttpClient>, Arc<ScriptHttp>, Arc<Credentials>) {
    let credentials = Arc::new(
        Credentials::open(Arc::new(StateStore::open(path).unwrap()), ApiOrigin::Pc).unwrap(),
    );
    rotation(
        &credentials,
        &["MUSIC_U=synthetic-message-user; Path=/; Secure"],
    );
    let http = Arc::new(ScriptHttp::new(replies));
    let api = Arc::new(
        MusicHttpClient::with_backend(
            credentials.clone(),
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http.clone(),
        )
        .unwrap(),
    );
    (api, http, credentials)
}

struct Fixture {
    root: tempfile::TempDir,
    client: MusicClient,
    session: Arc<AccountSession>,
    http: Arc<ScriptHttp>,
    credentials: Arc<Credentials>,
    store: Arc<StateStore>,
}
impl Fixture {
    fn new(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(&root.path().join("accounts"), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let (api, http, credentials) = transport(&root.path().join("cookie"), replies);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api, json!({"userId":71}), &session.token().unwrap())
            .unwrap();
        let client = MusicClient::with_session(session.clone(), registry);
        Self {
            root,
            client,
            session,
            http,
            credentials,
            store,
        }
    }

    fn start(&self) -> PcMessageConnectivity {
        self.client.start_pc_message_count_polling().unwrap()
    }

    fn paths(&self) -> Vec<String> {
        self.http
            .calls()
            .into_iter()
            .map(|call| call.path)
            .collect()
    }

    fn journal(&self, uid: &str) -> Option<Value> {
        self.store.get(uid, JOURNAL, "latest").unwrap()
    }
}

async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

async fn state(host: &PcMessageConnectivity, action: Action) -> Status {
    let result = host.message_read_all("1", action).await.unwrap();
    assert_eq!(result.epoch, "1");
    result.value
}

async fn open(host: &PcMessageConnectivity, http: &ScriptHttp, total: usize) {
    http.wait(total - 1).await;
    settle().await;
    host.drawer_visible(1, true).await.unwrap();
    http.wait(total).await;
    settle().await;
}

async fn prepare(host: &PcMessageConnectivity) -> String {
    let Status::Ready { intent } = state(host, Action::Prepare).await else {
        panic!("visible verified positive count did not allow preparation")
    };
    intent
}

#[tokio::test(start_paused = true)]
async fn confirmed_write_is_durable_before_send_and_retires_the_previous_count_read() {
    let (before, before_answer) = gate();
    let (clear, clear_answer) = gate();
    let f = Fixture::new([positive(5), positive(5), before, clear, positive(9)]);
    let host = f.start();
    open(&host, &f.http, 2).await;
    let view = host.subscribe();
    let cancelled = prepare(&host).await;
    assert_eq!(
        state(
            &host,
            Action::Cancel {
                intent: cancelled.clone()
            }
        )
        .await,
        Status::Idle
    );
    assert!(matches!(
        host.message_read_all("1", Action::Confirm { intent: cancelled })
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.paths(), [COUNT, COUNT]);
    assert!(f.journal("71").is_none());

    let intent = prepare(&host).await;
    host.refresh_counts(1).await.unwrap();
    f.http.wait(3).await;
    let confirm = host.message_read_all(
        "1",
        Action::Confirm {
            intent: intent.clone(),
        },
    );
    let inspect = async {
        f.http.wait(4).await;
        assert!(
            before_answer
                .send(Ok(reply(json!({"code":200,"msg":99}))))
                .is_err()
        );
        let Status::Pending { operation } = state(&host, Action::Status).await else {
            panic!("clear did not enter the durable pending phase")
        };
        assert_eq!(
            f.journal("71"),
            Some(json!({"operation":operation,"phase":"uncertain"}))
        );
        assert_eq!(view.borrow().counts.as_ref().unwrap().private, Some(5));
        assert!(matches!(
            host.message_read_all(
                "1",
                Action::Confirm {
                    intent: intent.clone()
                }
            )
            .await,
            Err(Error::MutationBusy)
        ));
        host.refresh_counts(1).await.unwrap();
        assert_eq!(
            state(&host, Action::Status).await,
            Status::Pending {
                operation: operation.clone()
            }
        );
        assert_eq!(f.paths(), [COUNT, COUNT, COUNT, CLEAR]);
        assert!(clear_answer.send(Ok(reply(json!({"code":200})))).is_ok());
        operation
    };
    let (ack, operation) = tokio::join!(confirm, inspect);
    assert_eq!(
        ack.unwrap().value,
        Status::Confirmed {
            operation: operation.clone()
        }
    );
    assert_eq!(
        f.journal("71"),
        Some(json!({"operation":operation,"phase":"confirmed"}))
    );
    assert_eq!(
        view.borrow().counts,
        Some(MessageCounts {
            private: Some(0),
            comments: Some(0),
            forwards: Some(0),
            notices: Some(0),
        })
    );
    assert!(matches!(
        host.message_read_all("1", Action::Confirm { intent }).await,
        Err(Error::StaleResponse)
    ));
    let calls = f.http.calls();
    let clear = &calls[3];
    let mut fields = clear.payload.as_object().unwrap().clone();
    assert!(fields.remove("header").is_some());
    assert!(
        fields.is_empty(),
        "clear's decrypted business payload must be exactly empty"
    );
    assert!(
        clear
            .cookie
            .as_deref()
            .is_some_and(|cookie| cookie.contains("MUSIC_U=synthetic-message-user"))
    );
    assert!(clear.login_chain.is_none() && clear.music_tu.is_none());

    host.refresh_counts(1).await.unwrap();
    f.http.wait(5).await;
    settle().await;
    assert_eq!(
        view.borrow().counts.as_ref().unwrap().private,
        Some(9),
        "a real later delivery is not masked"
    );
    assert_eq!(f.paths(), [COUNT, COUNT, COUNT, CLEAR, COUNT]);
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn lost_write_reply_reopens_readback_only_and_requires_all_four_explicit_zeros() {
    let f = Fixture::new([
        positive(5),
        positive(5),
        HttpResponse::Ready(Err(Error::Transport(
            "synthetic lost clear acknowledgement".into(),
        ))),
        positive(5),
        positive(5),
        positive(8),
        json!({"code":200,"msg":0,"comment":0,"forward":0}).into(),
        json!({"code":503}).into(),
        zero(),
    ]);
    let host = f.start();
    open(&host, &f.http, 2).await;
    let intent = prepare(&host).await;
    let Status::Uncertain { operation } = state(
        &host,
        Action::Confirm {
            intent: intent.clone(),
        },
    )
    .await
    else {
        panic!("lost write reply cannot be presented as a successful clear")
    };
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().private,
        Some(5)
    );
    assert!(matches!(
        host.message_read_all("1", Action::Confirm { intent }).await,
        Err(Error::StaleResponse)
    ));
    host.shutdown().await;

    let host = f.start();
    open(&host, &f.http, 5).await;
    assert_eq!(
        state(&host, Action::Prepare).await,
        Status::Uncertain {
            operation: operation.clone()
        }
    );
    let check = || Action::Check {
        operation: operation.clone(),
    };
    assert_eq!(
        state(&host, check()).await,
        Status::CheckInconclusive {
            operation: operation.clone()
        }
    );
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().private,
        Some(8)
    );
    assert_eq!(
        state(&host, check()).await,
        Status::CheckInconclusive {
            operation: operation.clone()
        }
    );
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().notices,
        None
    );
    assert_eq!(
        state(&host, check()).await,
        Status::CheckUnavailable {
            operation: operation.clone()
        }
    );
    assert_eq!(f.journal("71").unwrap()["phase"], "uncertain");
    assert_eq!(
        state(&host, check()).await,
        Status::CheckedClear { operation }
    );
    assert_eq!(f.journal("71").unwrap()["phase"], "checked_clear");
    assert_eq!(state(&host, Action::Prepare).await, Status::Unavailable);
    assert_eq!(
        f.paths(),
        [
            COUNT, COUNT, CLEAR, COUNT, COUNT, COUNT, COUNT, COUNT, COUNT
        ]
    );
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn account_switch_cancels_a_dispatched_write_and_keeps_only_the_old_owners_journal() {
    let (clear, answer) = gate();
    let f = Fixture::new([positive(5), positive(5), clear]);
    let host = f.start();
    open(&host, &f.http, 2).await;
    let intent = prepare(&host).await;
    let (foreign, foreign_http, _) = transport(&f.root.path().join("foreign"), [positive(27)]);
    let confirm = host.message_read_all("1", Action::Confirm { intent });
    let replace = async {
        f.http.wait(3).await;
        f.session
            .adopt(foreign, json!({"userId":72}), &f.session.token().unwrap())
            .unwrap();
        foreign_http.wait(1).await;
        settle().await;
        assert!(answer.send(Ok(reply(json!({"code":200})))).is_err());
    };
    let (result, ()) = tokio::join!(confirm, replace);
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(
        host.message_read_all("2", Action::Status)
            .await
            .unwrap()
            .value,
        Status::Idle
    );
    assert!(matches!(
        host.message_read_all("1", Action::Status).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().private,
        Some(27)
    );
    assert_eq!(f.journal("71").unwrap()["phase"], "uncertain");
    assert!(f.journal("72").is_none());
    assert_eq!(foreign_http.calls()[0].path, COUNT);
    assert_eq!(f.paths(), [COUNT, COUNT, CLEAR]);
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn confirmations_do_not_survive_new_observations_or_credential_scope_changes() {
    let f = Fixture::new([positive(5), positive(5), positive(5), positive(5)]);
    let host = f.start();
    open(&host, &f.http, 2).await;
    let first = prepare(&host).await;
    host.refresh_counts(1).await.unwrap();
    f.http.wait(3).await;
    settle().await;
    assert!(matches!(
        host.message_read_all("1", Action::Confirm { intent: first })
            .await,
        Err(Error::StaleResponse)
    ));

    let second = prepare(&host).await;
    rotation(
        &f.credentials,
        &["MUSIC_U=wrong-write-scope; Path=/eapi/msg/count/clear; Secure"],
    );
    assert!(matches!(
        host.message_read_all("1", Action::Confirm { intent: second })
            .await,
        Err(Error::CredentialChanged)
    ));
    host.refresh_counts(1).await.unwrap();
    f.http.wait(4).await;
    settle().await;
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().private,
        Some(5)
    );
    assert_eq!(state(&host, Action::Prepare).await, Status::Unavailable);
    assert!(f.journal("71").is_none());
    assert_eq!(f.paths(), [COUNT, COUNT, COUNT, COUNT]);
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn preparation_requires_the_open_drawer_and_expires_with_the_native_observation() {
    let (late, release) = gate();
    let f = Fixture::new([positive(5), positive(5), late, zero()]);
    let host = f.start();
    f.http.wait(1).await;
    settle().await;
    assert_eq!(state(&host, Action::Prepare).await, Status::Unavailable);
    host.drawer_visible(1, true).await.unwrap();
    f.http.wait(2).await;
    settle().await;
    let intent = prepare(&host).await;
    tokio::time::advance(Duration::from_secs(30)).await;
    f.http.wait(3).await;
    assert!(matches!(
        host.message_read_all("1", Action::Confirm { intent }).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(state(&host, Action::Prepare).await, Status::Unavailable);
    assert!(
        release
            .send(Ok(reply(
                json!({"code":200,"msg":8,"comment":0,"forward":0}),
            )))
            .is_ok()
    );
    settle().await;
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().private,
        Some(8)
    );
    assert_eq!(state(&host, Action::Prepare).await, Status::Unavailable);
    host.drawer_visible(1, false).await.unwrap();
    f.http.wait(4).await;
    assert_eq!(f.paths(), [COUNT, COUNT, COUNT, COUNT]);
    assert!(f.journal("71").is_none());
    host.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn dispatched_boundary_rechecks_credentials_after_waiting_for_the_native_http_gate() {
    let (held, release) = gate();
    let f = Fixture::new([positive(5), positive(5), held]);
    let host = f.start();
    open(&host, &f.http, 2).await;
    let intent = prepare(&host).await;
    // Stand in for another authenticated request using the same API transport.
    let other_read = f.client.message_counts("1");
    let write = async {
        f.http.wait(3).await;
        let confirm = host.message_read_all("1", Action::Confirm { intent });
        let revoke = async {
            let Status::Pending { operation } = state(&host, Action::Status).await else {
                panic!("expected the admitted clear to wait for the transport gate")
            };
            assert_eq!(f.journal("71").unwrap()["phase"], "uncertain");
            assert_eq!(f.paths(), [COUNT, COUNT, COUNT]);
            rotation(&f.credentials, &["MUSIC_U=new-revision; Path=/; Secure"]);
            assert!(
                release
                    .send(Ok(reply(json!({"code":200,"msg":11}))))
                    .is_ok()
            );
            operation
        };
        let (result, operation) = tokio::join!(confirm, revoke);
        assert_eq!(result.unwrap().value, Status::NotSent { operation });
    };
    let (_, ()) = tokio::join!(other_read, write);
    assert_eq!(f.journal("71").unwrap()["phase"], "not_sent");
    assert_eq!(
        host.subscribe().borrow().counts.as_ref().unwrap().private,
        Some(5)
    );
    assert_eq!(f.paths(), [COUNT, COUNT, COUNT]);
    host.shutdown().await;
}
