use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        messages::{MessageQuery, MessageReplyAction, MessageTab},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const PEER: &str = "9007199254740997";
const MESSAGE: &str = "9007199254741997";
const JOURNAL: &str = "private-message-reply-v1";
const SEND: &str = "/api/msg/private/send";
const BINDINGS: &str = "/api/w/v1/user/bindings/2";

struct Backend {
    store: Arc<StateStore>,
    phone: AtomicBool,
    sent: AtomicUsize,
    calls: Mutex<Vec<String>>,
}

impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls.lock().unwrap().push(path.into());
            match path {
                "/api/msg/private/users" => Ok(json!({"code":200,"more":false,"msgs":[
                    {"fromUser":{"userId":PEER,"nickname":"Synthetic peer"}}]})),
                "/api/msg/private/history" => {
                    assert_eq!(args, json!({"userId":PEER,"time":-1,"offset":0,"limit":10}));
                    Ok(json!({"code":200,"more":false,"msgs":[]}))
                }
                BINDINGS => {
                    assert_eq!(args, json!({"userId":"2"}));
                    Ok(
                        json!({"code":200,"bindings":if self.phone.load(Ordering::SeqCst) {
                        json!([{"type":1}])
                    } else {json!([{"type":0}])}}),
                    )
                }
                SEND => {
                    let attempt = self.sent.fetch_add(1, Ordering::SeqCst);
                    let journal = self.store.get("2", JOURNAL, PEER).unwrap().unwrap();
                    assert_eq!(journal["phase"], json!({"state":"unknown"}));
                    assert_eq!(journal.as_object().unwrap().len(), 2);
                    let text = match attempt {
                        0 => "  synthetic denied\n",
                        1 => "  synthetic deliver\n",
                        2 => "synthetic unknown",
                        _ => panic!("replayed synthetic message"),
                    };
                    assert_eq!(
                        args,
                        json!({"type":"text","id":0,"userIds":[PEER],"msg":text})
                    );
                    match attempt {
                        0 => Ok(
                            json!({"code":2202,"msg":"never display this synthetic upstream body"}),
                        ),
                        1 => Ok(
                            json!({"code":200,"newMsgs":[{"id":MESSAGE,"time":1700000100000_u64,
                            "fromUser":{"userId":2,"nickname":"Synthetic myself"},"toUserId":PEER,
                            "msg":{"type":6,"msg":"Synthetic canonical response"}}]}),
                        ),
                        _ => Err(Error::Transport("synthetic delivery reply lost".into())),
                    }
                }
                _ => panic!("unexpected message API: {path}"),
            }
        })
    }
}

fn client(api: Arc<Backend>, registry: Arc<ProfileRegistry>, uid: u64) -> (String, MusicClient) {
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
    (
        session.token().unwrap().generation.to_string(),
        MusicClient::with_session(session, registry),
    )
}
fn private() -> MessageQuery {
    MessageQuery {
        tab: MessageTab::Private,
        cursor: None,
        peer_id: None,
    }
}
fn thread() -> MessageQuery {
    MessageQuery {
        tab: MessageTab::Thread,
        cursor: None,
        peer_id: Some(PEER.into()),
    }
}
fn send(receipt: &str, text: &str) -> MessageReplyAction {
    MessageReplyAction::Send {
        receipt: receipt.into(),
        text: text.into(),
    }
}
fn status(receipt: &str) -> MessageReplyAction {
    MessageReplyAction::Status {
        receipt: receipt.into(),
    }
}
async fn observed(host: &MusicClient, epoch: &str) -> String {
    host.message_page(epoch, private()).await.unwrap();
    host.message_page(epoch, thread())
        .await
        .unwrap()
        .value
        .reply_receipt
        .unwrap()
}
async fn invoke(host: &MusicClient, epoch: &str, action: MessageReplyAction) -> Value {
    serde_json::to_value(host.message_reply(epoch, action).await.unwrap().value).unwrap()
}

#[tokio::test]
async fn one_shot_reply_keeps_the_peer_scope_and_never_recovers_from_a_matching_body() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend {
        store: registry.store(),
        phone: AtomicBool::new(false),
        sent: AtomicUsize::new(0),
        calls: Mutex::default(),
    });
    let (epoch, host) = client(api.clone(), registry.clone(), 2);
    assert!(
        host.message_page(&epoch, thread())
            .await
            .unwrap()
            .value
            .reply_receipt
            .is_none()
    );
    assert!(matches!(
        host.message_reply(&epoch, send(&"f".repeat(32), "synthetic"))
            .await,
        Err(Error::StaleResponse)
    ));
    let first = observed(&host, &epoch).await;
    assert!(matches!(
        host.message_reply(&epoch, send(&first, &"🪷".repeat(501)))
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        host.message_reply(&epoch, send(&first, "\u{00a0}\u{feff}\n"))
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        invoke(&host, &epoch, send(&first, &"🪷".repeat(500))).await["status"],
        "phone_required"
    );
    assert!(api.store.get("2", JOURNAL, PEER).unwrap().is_none());

    api.phone.store(true, Ordering::SeqCst);
    let denied = invoke(&host, &epoch, send(&first, "  synthetic denied\n")).await;
    assert_eq!(denied["status"], "denied");
    assert_eq!(denied["intercepted"], true);
    assert!(!denied.to_string().contains("synthetic upstream"));
    let second = denied["receipt"].as_str().unwrap();
    assert_ne!(first, second);
    let confirmed = invoke(&host, &epoch, send(second, "  synthetic deliver\n")).await;
    assert_eq!(confirmed["status"], "delivered");
    assert_eq!(confirmed["operation"], second);
    assert_eq!(confirmed["messageId"], MESSAGE);
    assert_eq!(confirmed["row"]["id"], MESSAGE);
    assert_eq!(confirmed["row"]["body"], "Synthetic canonical response");
    assert_eq!(confirmed["row"]["own"], true);
    let again = invoke(&host, &epoch, send(second, "do not replay")).await;
    assert_eq!(again["status"], "delivered");
    assert!(again.get("row").is_none());
    let third = confirmed["receipt"].as_str().unwrap();
    assert_ne!(second, third);

    let (restored_epoch, restored) = client(api.clone(), registry.clone(), 2);
    let restored_lease = observed(&restored, &restored_epoch).await;
    let local = invoke(&restored, &restored_epoch, status(&restored_lease)).await;
    assert_eq!(local["messageId"], MESSAGE);
    assert!(local.get("row").is_none());
    assert_eq!(
        api.store.get("2", JOURNAL, PEER).unwrap().unwrap()["phase"],
        json!({"state":"delivered","message_id":MESSAGE})
    );
    let lost = invoke(&host, &epoch, send(third, "synthetic unknown")).await;
    assert_eq!(lost["status"], "unknown");
    assert_eq!(lost["operation"], third);
    let recovered = invoke(&restored, &restored_epoch, status(&restored_lease)).await;
    assert_eq!(recovered["status"], "unknown");
    assert_eq!(recovered["operation"], third);
    assert_eq!(
        invoke(
            &restored,
            &restored_epoch,
            send(&restored_lease, "different text")
        )
        .await["status"],
        "unknown"
    );

    let (other_epoch, other) = client(api.clone(), registry, 3);
    let other_lease = observed(&other, &other_epoch).await;
    assert_eq!(
        invoke(&other, &other_epoch, status(&other_lease)).await["status"],
        "ready"
    );
    assert!(matches!(
        other.message_reply(&other_epoch, status(third)).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.sent.load(Ordering::SeqCst), 3);
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|path| path.as_str() == "/api/msg/private/history")
            .count(),
        4
    );
    assert_eq!(
        calls
            .iter()
            .filter(|path| path.as_str() == BINDINGS)
            .count(),
        4
    );
}
