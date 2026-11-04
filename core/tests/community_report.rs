mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{
            CommunityFeedAction, CommunityReportCommand as Command, ProfileReportReason as Reason,
        },
    },
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use support::{Reply, ScriptApi, gate};

const ID: &str = "90071992547409973331";
const INNER: &str = "90071992547409973332";
const FEED: &str = "/api/event/pc/history/feed/get";
const REPORT: &str = "/api/report/reportevent";
const DETAIL: &str = "/api/v1/event/73/90071992547409973331";
const RAW: &str = " \u{3000}合成动态举报 100%\n末行 \t";

fn row(id: &str, user: u64, kind: u32, body: Value) -> Value {
    json!({"id":id,"type":kind,"user":{"userId":user,"nickname":"合成作者"},
        "srcResId":"90071992547409971111","srcResType":"note","uuid":"synthetic-ignored","json":body.to_string()})
}
fn foreign() -> Value {
    row(
        ID,
        73,
        22,
        json!({"event":row(INNER,74,18,json!({"msg":"合成转发原文"}))}),
    )
}
fn feed(rows: Vec<Value>) -> Value {
    json!({"code":200,"data":{"events":rows,"more":false}})
}
fn open(root: &Path, api: Arc<ScriptApi>, uid: u64) -> (MusicClient, String) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    (MusicClient::with_session(session, registry), epoch)
}
fn data(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()["value"].clone()
}
async fn first(client: &MusicClient, epoch: &str) -> Value {
    data(
        client
            .community_feed(epoch, CommunityFeedAction::First)
            .await
            .unwrap(),
    )
}
fn key(value: &Value) -> &str {
    value["key"].as_str().unwrap()
}
async fn view(client: &MusicClient, epoch: &str, key: &str, command: Command) -> Value {
    data(
        client
            .community_note_report(epoch, key, command)
            .await
            .unwrap(),
    )
}
async fn draft(client: &MusicClient, epoch: &str, key: &str, reason: Reason) -> String {
    let dto = view(client, epoch, key, Command::Prepare { reason }).await;
    assert_eq!(dto["status"], "ready");
    dto["receipt"].as_str().unwrap().into()
}

#[tokio::test]
async fn uses_the_outer_event_and_author_and_retains_lost_response_across_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let mut anonymous = row("81", 83, 18, json!({"msg":"合成匿名"}));
    anonymous["anonymityInfo"] = json!({"encryptUserId":"opaque"});
    let api = Arc::new(ScriptApi::new([
        feed(vec![
            foreign(),
            row("82", 2, 18, json!({"msg":"本人"})),
            anonymous,
        ])
        .into(),
        json!({"code":200,"event":foreign()}).into(),
        Reply::Ready(Err(Error::Transport(
            "synthetic report response lost".into(),
        ))),
    ]));
    let (client, epoch) = open(tmp.path(), api.clone(), 2);
    let visible = first(&client, &epoch).await;
    let guest = &visible["cards"][0];
    assert_eq!(guest["canReport"], true);
    for row in [
        &visible["cards"][1],
        &visible["cards"][2],
        &guest["attachment"]["card"],
    ] {
        assert_ne!(row["canReport"], true);
        assert!(
            client
                .community_note_report(&epoch, key(row), Command::Inspect)
                .await
                .is_err()
        );
    }
    let cancel = draft(&client, &epoch, key(guest), Reason::Political).await;
    view(
        &client,
        &epoch,
        key(guest),
        Command::Cancel {
            receipt: cancel.clone(),
        },
    )
    .await;
    assert!(
        client
            .community_note_report(&epoch, key(guest), Command::Submit { receipt: cancel })
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 1);
    let receipt = draft(
        &client,
        &epoch,
        key(guest),
        Reason::Other {
            description: RAW.into(),
        },
    )
    .await;
    assert_eq!(
        view(
            &client,
            &epoch,
            key(guest),
            Command::Submit {
                receipt: receipt.clone()
            }
        )
        .await["status"],
        "uncertain"
    );
    assert_eq!(
        view(&client, &epoch, key(guest), Command::Submit { receipt }).await["status"],
        "uncertain"
    );
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [FEED, DETAIL, REPORT]
    );
    assert_eq!(calls[1].payload, json!({}));
    assert_eq!(
        calls[2].payload,
        json!({"id":ID,"userid":"73","reason":RAW})
    );
    assert!(calls.iter().all(|call| !call.cache));
    drop(client);
    let reopen = Arc::new(ScriptApi::new([feed(vec![foreign()]).into()]));
    let (client, epoch) = open(tmp.path(), reopen.clone(), 2);
    let page = first(&client, &epoch).await;
    let next = key(&page["cards"][0]);
    assert_ne!(next, key(guest));
    assert_eq!(
        view(
            &client,
            &epoch,
            next,
            Command::Prepare {
                reason: Reason::Political
            }
        )
        .await,
        json!({"status":"uncertain","receipt":null})
    );
    assert_eq!(reopen.calls().len(), 1);
    let store = ProfileRegistry::open(tmp.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    assert_eq!(
        store.get("2", "community-event-report-v1", ID).unwrap(),
        Some(json!("uncertain"))
    );
    let other = Arc::new(ScriptApi::new([feed(vec![foreign()]).into()]));
    let (client, epoch) = open(tmp.path(), other.clone(), 3);
    let page = first(&client, &epoch).await;
    assert_eq!(
        view(&client, &epoch, key(&page["cards"][0]), Command::Inspect).await["status"],
        "ready"
    );
    assert_eq!(other.calls().len(), 1);
}

#[tokio::test]
async fn old_confirmation_cannot_attach_to_a_replaced_handle_or_mismatched_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        feed(vec![foreign()]).into(),
        feed(vec![foreign()]).into(),
        json!({"code":200,"event":row(ID,74,18,json!({"msg":"不同作者"}))}).into(),
        json!({"code":200,"event":foreign()}).into(),
        json!({"code":-1}).into(),
    ]));
    let (client, epoch) = open(tmp.path(), api.clone(), 2);
    let original = first(&client, &epoch).await;
    let original = key(&original["cards"][0]);
    let stale = draft(&client, &epoch, original, Reason::HarmfulContent).await;
    let replaced = data(
        client
            .community_feed(&epoch, CommunityFeedAction::Refresh)
            .await
            .unwrap(),
    );
    let replaced = key(&replaced["cards"][0]);
    assert_ne!(original, replaced);
    assert!(
        client
            .community_note_report(&epoch, replaced, Command::Submit { receipt: stale })
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 2);
    let mismatched = draft(&client, &epoch, replaced, Reason::PersonalAttack).await;
    assert_eq!(
        view(
            &client,
            &epoch,
            replaced,
            Command::Submit {
                receipt: mismatched
            }
        )
        .await["status"],
        "target_unavailable"
    );
    assert_eq!(api.calls().len(), 3);
    let explicit = draft(&client, &epoch, replaced, Reason::PersonalAttack).await;
    assert_eq!(
        view(
            &client,
            &epoch,
            replaced,
            Command::Submit { receipt: explicit }
        )
        .await["status"],
        "duplicate"
    );
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [FEED, FEED, DETAIL, DETAIL, REPORT]
    );
    assert_eq!(
        calls[4].payload,
        json!({"id":ID,"userid":"73","reason":"恶意人身攻击"})
    );
}

#[tokio::test]
async fn refresh_during_target_read_prevents_dispatch() {
    let tmp = tempfile::tempdir().unwrap();
    let (pending, release) = gate();
    let api = Arc::new(ScriptApi::new([
        feed(vec![foreign()]).into(),
        pending,
        feed(vec![foreign()]).into(),
    ]));
    let (client, epoch) = open(tmp.path(), api.clone(), 2);
    let page = first(&client, &epoch).await;
    let key = key(&page["cards"][0]);
    let receipt = draft(&client, &epoch, key, Reason::Advertising).await;
    let (result, _) = tokio::join!(
        client.community_note_report(&epoch, key, Command::Submit { receipt }),
        async {
            while api.calls().len() < 2 {
                tokio::task::yield_now().await;
            }
            client
                .community_feed(&epoch, CommunityFeedAction::Refresh)
                .await
                .unwrap();
            release
                .send(Ok(json!({"code":200,"event":foreign()})))
                .unwrap();
        },
    );
    assert!(matches!(result, Err(Error::StaleResponse)));
    assert_eq!(
        api.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [FEED, DETAIL, FEED]
    );
}
