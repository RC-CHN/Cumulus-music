use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
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

const BINDINGS: &str = "/api/w/v1/user/bindings/41";
const SHARE: &str = "/api/share/friends/resource";
const JOURNAL: &str = "community-composer-publication-v1";
const NOTE: &str = "9007199254740997";

struct Backend {
    store: Arc<StateStore>,
    calls: Mutex<Vec<String>>,
    phone: AtomicBool,
    shares: AtomicUsize,
}

fn event(owner: u64, uuid: &Value) -> Value {
    json!({"id":NOTE,"user":{"userId":owner,"nickname":"Synthetic account"},
      "uuid":uuid,"type":18,"json":"{\"msg\":\"Synthetic confirmed note\",\"song\":{\"id\":99,\"name\":\"Track\"}}"})
}

impl Api for Backend {
    fn local_device_id(&self) -> cumulus_core::Result<Option<String>> {
        Ok(Some("Synthetic 本机/device".into()))
    }
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls.lock().unwrap().push(path.into());
            match path {
                "/api/search/song/list/page" => Ok(json!({"code":200,"data":{"resources":[
                    {"baseInfo":{"simpleSongData":{"id":99,"name":"Synthetic track"}}}
                ]}})),
                BINDINGS => {
                    assert_eq!(args, json!({"userId":"41"}));
                    let bindings = if self.phone.load(Ordering::SeqCst) {
                        json!([{"type":1,"tokenJsonStr":"{\"cellphone\":\"18800000000\"}"}])
                    } else {
                        json!([{"type":0}])
                    };
                    Ok(json!({"code":200,"bindings":bindings}))
                }
                SHARE => {
                    let attempt = self.shares.fetch_add(1, Ordering::SeqCst);
                    let uuid = args["uuid"].as_str().unwrap();
                    assert_eq!(uuid.len(), 32);
                    assert!(
                        uuid.bytes()
                            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
                    );
                    let journal = self.store.get("41", JOURNAL, "latest").unwrap().unwrap();
                    assert_eq!(journal["uuid"], uuid);
                    assert_eq!(journal["phase"]["state"], "pending");
                    assert!(!journal.to_string().contains("Synthetic"));
                    assert_eq!(
                        args,
                        json!({"id":"99","msg":if attempt==0 {"Synthetic confirmed note"} else {"Synthetic unknown note"},
                            "type":"song","uuid":uuid,"active":0,"pics":"[]"})
                    );
                    if attempt == 0 {
                        Ok(json!({"code":200,"event":event(41, &args["uuid"])}))
                    } else {
                        Err(Error::Transport("synthetic remote reply lost".into()))
                    }
                }
                path if path == format!("/api/v1/event/41/{NOTE}") => {
                    assert_eq!(args, json!({}));
                    Ok(json!({"code":200,"event":event(41, &Value::Null)}))
                }
                _ => panic!("unexpected synthetic route {path}"),
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

async fn draft(host: &MusicClient, epoch: &str, text: &str) -> String {
    let search = host
        .community_composer_search(epoch, ComposerKind::Song, "synthetic", 0)
        .await
        .unwrap();
    let rows = serde_json::to_value(search.value).unwrap();
    let receipt = rows["items"][0]["receipt"].as_str().unwrap().to_owned();
    let reply = host
        .community_composer_draft(
            epoch,
            ComposerDraftAction::Save {
                revision: None,
                text: text.into(),
                attachment: DraftAttachmentEdit::Select { receipt },
            },
        )
        .unwrap();
    serde_json::to_value(reply.value).unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn phone_gate_acknowledgement_and_unknown_reopen_use_the_intended_account_only() {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend {
        store: registry.store(),
        calls: Mutex::default(),
        phone: AtomicBool::new(false),
        shares: AtomicUsize::new(0),
    });
    let (epoch, host) = client(api.clone(), registry.clone(), 41);
    let status = || ComposerPublishAction::Status;
    let submit = |revision: &str| ComposerPublishAction::Submit {
        revision: revision.into(),
    };
    assert!(matches!(
        host.community_composer_publish(&epoch, status())
            .await
            .unwrap()
            .value,
        ComposerPublication::Ready
    ));
    let first = draft(&host, &epoch, "Synthetic confirmed note").await;
    assert!(matches!(
        host.community_composer_publish(&epoch, submit(&first))
            .await
            .unwrap()
            .value,
        ComposerPublication::PhoneRequired
    ));
    assert!(api.store.get("41", JOURNAL, "latest").unwrap().is_none());
    assert_eq!(api.shares.load(Ordering::SeqCst), 0);
    api.phone.store(true, Ordering::SeqCst);
    let confirmed = host
        .community_composer_publish(&epoch, submit(&first))
        .await
        .unwrap()
        .value;
    let ComposerPublication::Published {
        revision,
        event_id,
        detail,
    } = confirmed
    else {
        panic!("expected definitive event acknowledgement")
    };
    assert_eq!(event_id, NOTE);
    assert_eq!(revision, first);
    assert!(
        host.community_composer_draft(&epoch, ComposerDraftAction::Read)
            .unwrap()
            .value
            .is_none()
    );
    let repeat = host
        .community_composer_publish(&epoch, submit(&first))
        .await
        .unwrap()
        .value;
    assert!(matches!(repeat, ComposerPublication::Published { detail: old, .. } if old == detail));

    // A separate native host can restore the exact durable receipt and its
    // guarded detail capability without another publication or feed scan.
    let (reopen_epoch, reopened) = client(api.clone(), registry.clone(), 41);
    assert!(
        matches!(reopened.community_composer_publish(&reopen_epoch, status()).await.unwrap().value,
        ComposerPublication::Published { detail: previous, .. } if previous == detail)
    );
    assert!(
        reopened
            .community_detail(&reopen_epoch, &detail)
            .await
            .unwrap()
            .value
            .is_some()
    );

    let second = draft(&host, &epoch, "Synthetic unknown note").await;
    assert!(
        matches!(host.community_composer_publish(&epoch, submit(&second)).await.unwrap().value,
        ComposerPublication::Uncertain { revision } if revision == second)
    );
    assert!(
        matches!(reopened.community_composer_publish(&reopen_epoch, status()).await.unwrap().value,
        ComposerPublication::Uncertain { revision } if revision == second)
    );
    assert!(
        matches!(reopened.community_composer_publish(&reopen_epoch, submit(&second)).await.unwrap().value,
        ComposerPublication::Uncertain { revision } if revision == second)
    );
    assert!(matches!(
        host.community_composer_draft(&epoch, ComposerDraftAction::Discard { revision: second }),
        Err(Error::MutationBusy)
    ));
    assert!(
        reopened
            .community_composer_draft(&reopen_epoch, ComposerDraftAction::Read)
            .unwrap()
            .value
            .is_some()
    );

    let (foreign_epoch, foreign) = client(api.clone(), registry, 42);
    assert!(matches!(
        foreign
            .community_composer_publish(&foreign_epoch, status())
            .await
            .unwrap()
            .value,
        ComposerPublication::Ready
    ));
    assert!(matches!(
        foreign.community_detail(&foreign_epoch, &detail).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.shares.load(Ordering::SeqCst), 2);
    assert_eq!(
        *api.calls.lock().unwrap(),
        [
            "/api/search/song/list/page",
            BINDINGS,
            BINDINGS,
            SHARE,
            &format!("/api/v1/event/41/{NOTE}"),
            "/api/search/song/list/page",
            BINDINGS,
            SHARE,
        ]
    );
}
