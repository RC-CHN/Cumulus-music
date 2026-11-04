mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::CommunityFeedAction as Action},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const FEED: &str = "/api/event/pc/history/feed/get";
const PROFILE: &str = "/api/w/v1/user/detail/73";
const NOTES: &str = "/api/event/get/73";
const DETAIL: &str = "/api/v1/event/73/991";
const COMMENTS: &str = "/api/v1/resource/comments/A_EV_2_991_73";

fn fixture(
    replies: impl IntoIterator<Item = Reply>,
) -> (
    tempfile::TempDir,
    Arc<ScriptApi>,
    Arc<AccountSession>,
    Arc<MusicClient>,
    String,
) {
    let directory = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (directory, api, session, client, epoch)
}

fn card(id: u32, owner: u32) -> Value {
    json!({"id":id,"type":18,"eventTime":1700000000000_u64,
        "user":{"userId":owner,"nickname":"合成作者"},
        "info":{"threadId":format!("A_EV_2_{id}_{owner}")},
        "json":json!({"msg":format!("合成笔记 {id}"),"credentials":"PRIVATE"}).to_string()})
}
fn header(blocked: bool) -> Value {
    json!({"code":200,"profile":{"userId":73,"nickname":"合成作者","eventCount":25,"inBlacklist":blocked}})
}
fn data<T: serde::Serialize>(value: T) -> Value {
    to_value(value).unwrap()["value"].clone()
}
fn next(view: &Value) -> Action {
    Action::Next {
        continuation: view["continuation"].as_str().unwrap().into(),
    }
}

#[tokio::test]
async fn read_profile_notes_preserves_numeric_cursor_and_separates_followed_handles() {
    let mut anonymous = card(993, 73);
    anonymous["anonymityInfo"] = json!({"encryptUserId":"PRIVATE"});
    let (_directory, api, _, client, epoch) = fixture([
        json!({"code":200,"data":{"events":[card(700, 99)],"more":false}}).into(),
        header(false).into(),
        json!({"code":200,"events":[card(991, 73),card(992, 74),anonymous],"more":true,"lasttime":"1700000000000","size":25}).into(),
        json!({"code":200,"event":card(991, 73)}).into(),
        json!({"code":200,"comments":[],"more":false,"total":0}).into(),
        header(false).into(),
        json!({"code":200,"events":[card(994, 73)],"more":false}).into(),
        json!({"code":200,"event":card(700, 99)}).into(),
    ]);
    let follow = data(client.community_feed(&epoch, Action::First).await.unwrap());
    let initial = data(
        client
            .profile_notes(&epoch, "73", Action::First)
            .await
            .unwrap(),
    );
    assert_eq!(initial["cards"].as_array().unwrap().len(), 1);
    assert_eq!(initial["cards"][0]["message"], "合成笔记 991");
    assert!(initial["replace"].as_bool().unwrap());
    assert!(!initial.to_string().contains("PRIVATE"));
    let detail = initial["cards"][0]["detail"].as_str().unwrap();
    assert_eq!(
        data(client.community_detail(&epoch, detail).await.unwrap())["commentsAvailable"],
        true
    );
    assert_eq!(
        data(client.community_comments(&epoch, detail, 0).await.unwrap())["total"],
        0
    );
    assert!(matches!(
        client.profile_notes(&epoch, "74", next(&initial)).await,
        Err(Error::StaleResponse)
    ));
    let after = data(
        client
            .profile_notes(&epoch, "73", next(&initial))
            .await
            .unwrap(),
    );
    assert_eq!(after["cards"][0]["message"], "合成笔记 994");
    assert_eq!(after["replace"], false);
    assert!(after["continuation"].is_null());
    let follow_key = follow["cards"][0]["detail"].as_str().unwrap();
    assert_eq!(
        data(client.community_detail(&epoch, follow_key).await.unwrap())["card"]["message"],
        "合成笔记 700"
    );
    let calls = api.calls();
    assert!(calls.iter().all(|call| !call.cache));
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            FEED,
            PROFILE,
            NOTES,
            DETAIL,
            COMMENTS,
            PROFILE,
            NOTES,
            "/api/v1/event/99/700"
        ]
    );
    assert_eq!(calls[1].payload, json!({"all":true,"userId":"73"}));
    assert_eq!(
        calls[2].payload,
        json!({"offset":0,"limit":20,"getcounts":true,"time":-1})
    );
    assert_eq!(
        calls[6].payload,
        json!({"offset":0,"limit":20,"getcounts":true,"time":1700000000000_u64})
    );
}

#[tokio::test]
async fn revocation_is_distinct_from_empty_and_revokes_current_handles() {
    let (_directory, api, _, client, epoch) = fixture([
        header(false).into(),
        json!({"code":200,"events":[card(991, 73)],"more":true,"lasttime":900}).into(),
        header(false).into(),
        json!({"code":200,"events":[],"more":true,"lasttime":900}).into(),
        header(true).into(),
        json!({"code":200,"events":[],"more":false}).into(),
    ]);
    let initial = data(
        client
            .profile_notes(&epoch, "73", Action::First)
            .await
            .unwrap(),
    );
    assert!(matches!(
        client.profile_notes(&epoch, "73", next(&initial)).await,
        Err(Error::Response(_))
    ));
    let revoked = data(
        client
            .profile_notes(&epoch, "73", next(&initial))
            .await
            .unwrap(),
    );
    assert_eq!(
        revoked,
        json!({"cards":[],"continuation":null,"replace":true,"unavailable":true})
    );
    assert!(matches!(
        client
            .community_detail(&epoch, initial["cards"][0]["detail"].as_str().unwrap())
            .await,
        Err(Error::StaleResponse)
    ));
    let own = data(
        client
            .profile_notes(&epoch, "9", Action::First)
            .await
            .unwrap(),
    );
    assert_eq!(own, json!({"cards":[],"continuation":null,"replace":true}));
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [PROFILE, NOTES, PROFILE, NOTES, PROFILE, "/api/event/get/9"]
    );
}

#[tokio::test]
async fn switching_account_during_guest_header_never_starts_the_notes_request() {
    let (reply, release) = gate();
    let (_directory, api, session, client, epoch) = fixture([reply]);
    let held = tokio::spawn(async move { client.profile_notes(&epoch, "73", Action::First).await });
    api.wait_for_calls(1).await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    release.send(Ok(header(false))).unwrap();
    assert!(matches!(held.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls().len(), 1);
}

#[tokio::test]
async fn a_new_profile_visit_does_not_send_notes_for_the_superseded_target() {
    let (reply, release) = gate();
    let (_directory, api, _, client, epoch) = fixture([
        reply,
        json!({"code":200,"profile":{"userId":74,"nickname":"第二个资料"}}).into(),
        json!({"code":200,"events":[card(997,74)],"more":false}).into(),
    ]);
    let held = {
        let client = client.clone();
        let epoch = epoch.clone();
        tokio::spawn(async move { client.profile_notes(&epoch, "73", Action::First).await })
    };
    api.wait_for_calls(1).await;
    let second = data(
        client
            .profile_notes(&epoch, "74", Action::First)
            .await
            .unwrap(),
    );
    assert_eq!(second["cards"][0]["message"], "合成笔记 997");
    release.send(Ok(header(false))).unwrap();
    assert!(matches!(held.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        api.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [PROFILE, "/api/w/v1/user/detail/74", "/api/event/get/74"]
    );
}
