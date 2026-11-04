mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{
            CommunityDeleteAction as Action, CommunityFeedAction, CommunityTopicAction,
            ComposerPublishAction,
        },
    },
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const EVENT: &str = "90071992547409973339";
const FEED: &str = "/api/event/pc/history/feed/get";
const DELETE: &str = "/api/event/delete";

fn open(
    root: &std::path::Path,
    api: Arc<ScriptApi>,
    uid: u64,
) -> (Arc<MusicClient>, Arc<AccountSession>, String) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    (
        Arc::new(MusicClient::with_session(session.clone(), registry)),
        session,
        epoch,
    )
}
fn row(id: &str, owner: u64, kind: u32, body: Value) -> Value {
    json!({"id":id,"type":kind,"user":{"userId":owner,"nickname":"合成作者"},"json":body.to_string()})
}
fn own() -> Value {
    row(EVENT, 9, 18, json!({"msg":"自有合成笔记"}))
}
fn feed(events: Vec<Value>) -> Value {
    json!({"code":200,"data":{"events":events,"more":false}})
}
fn data(value: impl serde::Serialize) -> Value {
    to_value(value).unwrap()["value"].clone()
}
fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap()
}
async fn first(client: &MusicClient, epoch: &str) -> Value {
    data(
        client
            .community_feed(epoch, CommunityFeedAction::First)
            .await
            .unwrap(),
    )
}
async fn ticket(client: &MusicClient, epoch: &str, card: &str) -> Action {
    let inspected = data(
        client
            .community_note_delete(epoch, card, Action::Inspect)
            .await
            .unwrap(),
    );
    assert_eq!(inspected["status"], "ready");
    Action::Confirm {
        confirmation: field(&inspected, "confirmation").into(),
    }
}

#[tokio::test]
async fn exact_own_confirmation_retires_native_leases_and_preserves_publication_history() {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        feed(vec![own(), row("71", 8, 18, json!({"msg":"来宾"}))]).into(),
        json!({"code":200,"events":[own()],"more":false}).into(),
        json!({"code":200,"act":{"actId":"133"}}).into(),
        json!({"code":200,"events":[own()],"more":false}).into(),
        json!({"code":200}).into(),
        feed(vec![own(), row("71", 8, 18, json!({"msg":"来宾"}))]).into(),
    ]));
    let (client, session, epoch) = open(dir.path(), api.clone(), 9);
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let published = uuid::Uuid::new_v4().to_string();
    let revision = uuid::Uuid::new_v4().to_string();
    store
        .put(
            "9",
            "community-composer-publication-v1",
            "latest",
            &json!({
                "revision":revision,"uuid":"00000000000000000000000000000001",
                "phase":{"state":"confirmed","event":EVENT,"detail":published}
            }),
        )
        .unwrap();
    let publication = data(
        client
            .community_composer_publish(&epoch, ComposerPublishAction::Status)
            .await
            .unwrap(),
    );
    assert_eq!(publication["status"], "published");
    let cards = first(&client, &epoch).await;
    let profile = data(
        client
            .profile_notes(&epoch, "9", CommunityFeedAction::First)
            .await
            .unwrap(),
    );
    let topic = data(client.community_topic(&epoch, "合成话题").await.unwrap());
    let topic_cards = data(
        client
            .community_topic_slice(&epoch, field(&topic, "handle"), CommunityTopicAction::First)
            .await
            .unwrap(),
    );
    let own = field(&cards["cards"][0], "detail");
    let guest = field(&cards["cards"][1], "detail");
    assert_eq!(cards["cards"][0]["canDelete"], true);
    assert!(cards["cards"][1].get("canDelete").is_none());
    let confirm = ticket(&client, &epoch, own).await;
    assert!(
        client
            .community_note_delete(&epoch, guest, Action::Inspect)
            .await
            .is_err()
    );
    assert!(
        client
            .community_note_delete(
                &epoch,
                own,
                Action::Confirm {
                    confirmation: uuid::Uuid::new_v4().to_string()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 4);
    let result = data(
        client
            .community_note_delete(&epoch, own, confirm)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "acknowledged");
    let retired = result["retired"].as_array().unwrap();
    for key in [
        own,
        field(&profile["cards"][0], "detail"),
        field(&topic_cards["cards"][0], "detail"),
        published.as_str(),
    ] {
        assert!(retired.contains(&json!(key)));
        assert!(client.community_detail(&epoch, key).await.is_err());
    }
    let publication = data(
        client
            .community_composer_publish(&epoch, ComposerPublishAction::Status)
            .await
            .unwrap(),
    );
    assert_eq!(publication, json!({"status":"removed","revision":revision}));
    assert_eq!(
        store
            .get("9", "community-composer-publication-v1", "latest")
            .unwrap()
            .unwrap()["phase"]["state"],
        "confirmed"
    );
    let next = first(&client, &epoch).await;
    assert_eq!(next["cards"].as_array().unwrap().len(), 1);
    assert_eq!(next["cards"][0]["message"], "来宾");
    assert_eq!(api.calls()[4].path, DELETE);
    assert_eq!(api.calls()[4].payload, json!({"id":EVENT}));
    assert!(api.calls().iter().all(|call| !call.cache));
    session.detach().unwrap();
    assert!(
        client
            .community_note_delete(&epoch, own, Action::Check)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn unknown_result_survives_reopen_and_only_exact_tombstone_changes_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let first_api = Arc::new(ScriptApi::new([
        feed(vec![own()]).into(),
        Reply::Ready(Err(Error::Transport("synthetic lost response".into()))),
    ]));
    {
        let (client, _, epoch) = open(dir.path(), first_api.clone(), 9);
        let cards = first(&client, &epoch).await;
        let card = field(&cards["cards"][0], "detail");
        let confirm = ticket(&client, &epoch, card).await;
        assert_eq!(
            data(
                client
                    .community_note_delete(&epoch, card, confirm)
                    .await
                    .unwrap()
            )["status"],
            "uncertain"
        );
    }
    assert_eq!(first_api.calls()[1].path, DELETE);
    let second = Arc::new(ScriptApi::new([
        feed(vec![own()]).into(),
        json!({"code":404}).into(),
        json!({"code":200,"event":row(EVENT,8,22,json!({}))}).into(),
        json!({"code":200,"event":row(EVENT,9,22,json!({"song":{"id":99,"name":"Not a tombstone"}}))}).into(),
        json!({"code":200,"event":row(EVENT,9,22,json!({"event":row("85",8,18,json!({}))}))})
            .into(),
        json!({"code":200,"event":row(EVENT,9,22,json!({}))}).into(),
    ]));
    let (client, _, epoch) = open(dir.path(), second.clone(), 9);
    let cards = first(&client, &epoch).await;
    let card = field(&cards["cards"][0], "detail");
    let prior = data(
        client
            .community_note_delete(&epoch, card, Action::Inspect)
            .await
            .unwrap(),
    );
    assert_eq!(prior["status"], "uncertain");
    assert!(prior.get("confirmation").is_none());
    assert_eq!(
        data(
            client
                .community_note_delete(
                    &epoch,
                    card,
                    Action::Confirm {
                        confirmation: uuid::Uuid::new_v4().to_string()
                    }
                )
                .await
                .unwrap()
        )["status"],
        "uncertain"
    );
    for expected in [
        "unavailable",
        "unavailable",
        "unavailable",
        "still_visible",
        "observed_removed",
    ] {
        let observed = data(
            client
                .community_note_delete(&epoch, card, Action::Check)
                .await
                .unwrap(),
        );
        assert_eq!(observed["status"], expected);
    }
    let calls = second.calls();
    assert_eq!(calls[0].path, FEED);
    assert!(
        calls[1..]
            .iter()
            .all(|call| call.path == format!("/api/v1/event/9/{EVENT}")
                && call.payload == json!({})
                && !call.cache)
    );
}

#[tokio::test]
async fn account_change_during_delete_does_not_publish_an_old_receipt_or_replay() {
    let dir = tempfile::tempdir().unwrap();
    let (pending, release) = gate();
    let api = Arc::new(ScriptApi::new([feed(vec![own()]).into(), pending]));
    let (client, session, epoch) = open(dir.path(), api.clone(), 9);
    let cards = first(&client, &epoch).await;
    let card = field(&cards["cards"][0], "detail").to_owned();
    let confirm = ticket(&client, &epoch, &card).await;
    let in_flight = tokio::spawn({
        let client = client.clone();
        let epoch = epoch.clone();
        async move { client.community_note_delete(&epoch, &card, confirm).await }
    });
    api.wait_for_calls(2).await;
    let new = Arc::new(ScriptApi::new([feed(vec![own()]).into()]));
    session
        .adopt(new.clone(), json!({"userId":8}), &session.token().unwrap())
        .unwrap();
    release.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(
        in_flight.await.unwrap(),
        Err(Error::StaleResponse)
    ));
    let epoch = session.token().unwrap().generation.to_string();
    let cards = first(&client, &epoch).await;
    assert!(cards["cards"][0].get("canDelete").is_none());
    assert_eq!(new.calls().len(), 1);
}
