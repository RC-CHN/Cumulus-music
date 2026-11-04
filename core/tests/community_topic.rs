mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::CommunityTopicAction as Action},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const DETAIL: &str = "/api/act/detail";
const HOT: &str = "/api/act/event/hot";
const ALL: &str = "/api/act/event";
const NOTICE: &str = "/api/act/event/new";
const TOPIC: &str = "听歌 100%";
const ACT: &str = "90071992547409999";
const CURSOR: &str = "90071992547409933331";

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
fn meta(id: Value) -> Value {
    json!({"code":200,"act":{"actId":id,"title":"ignored server title","text":["合成说明","不可投影"],
        "coverPCUrl":"https://p1.music.126.net/cover.png?token=secret",
        "coverPCLongUrl":"https://unknown.invalid/private", "participateCount":27,
        "resourceJson":"DO_NOT_EXPORT","resourceType":4}})
}
fn row(id: u32, owner: u32) -> Value {
    json!({"id":id,"type":18,"eventTime":1700000000000_u64,"user":{"userId":owner,"nickname":"合成作者"},
        "actId":ACT,"info":{"threadId":format!("A_EV_2_{id}_{owner}")},
        "json":json!({"msg":format!("合成笔记 {id}")}).to_string()})
}
fn events(rows: Vec<Value>, more: bool, cursor: Value) -> Value {
    json!({"code":200,"events":rows,"more":more,"lasttime":cursor})
}
fn data(value: impl serde::Serialize) -> Value {
    to_value(value).unwrap()["value"].clone()
}
fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap()
}
fn next(value: &Value) -> Action {
    Action::Next {
        continuation: field(value, "continuation").into(),
    }
}

#[tokio::test]
async fn exact_topic_requests_preserve_cursor_json_type_and_do_not_lose_retry_position() {
    let (_dir, api, _, client, epoch) = fixture([
        meta(json!(ACT)).into(),
        json!({"code":200,"events":[row(1,73)]}).into(),
        events(vec![row(1, 73), row(2, 73)], true, json!(CURSOR)).into(),
        json!({"code":503}).into(),
        events(vec![row(3, 73)], true, json!(9007199254740997_u64)).into(),
        events(vec![row(4, 73)], false, Value::Null).into(),
        json!({"code":200,"new":true}).into(),
        events(vec![row(5, 73)], false, Value::Null).into(),
        json!({"code":200,"events":[]}).into(),
    ]);
    let opened = data(client.community_topic(&epoch, TOPIC).await.unwrap());
    assert_eq!(opened["title"], format!("#{TOPIC}#"));
    assert_eq!(opened["description"], "合成说明");
    assert_eq!(opened["wideCoverUrl"], Value::Null);
    assert_eq!(opened["participants"], 27);
    assert!(!opened.to_string().contains("secret"));
    assert!(!opened.to_string().contains("DO_NOT_EXPORT"));
    assert!(!opened.to_string().contains(ACT));
    let key = field(&opened, "handle");
    let hot = data(
        client
            .community_topic_slice(&epoch, key, Action::Hot)
            .await
            .unwrap(),
    );
    let first = data(
        client
            .community_topic_slice(&epoch, key, Action::First)
            .await
            .unwrap(),
    );
    assert_eq!(hot["cards"][0]["key"], first["cards"][0]["key"]);
    assert!(hot["continuation"].is_null());
    assert!(
        client
            .community_topic_slice(&epoch, key, next(&first))
            .await
            .is_err()
    );
    let later = data(
        client
            .community_topic_slice(&epoch, key, next(&first))
            .await
            .unwrap(),
    );
    assert!(
        client
            .community_topic_slice(&epoch, key, next(&first))
            .await
            .is_err()
    );
    let final_page = data(
        client
            .community_topic_slice(&epoch, key, next(&later))
            .await
            .unwrap(),
    );
    assert!(final_page["continuation"].is_null());
    assert!(
        client
            .community_topic_notice(&epoch, key)
            .await
            .unwrap()
            .value
    );
    assert_eq!(
        data(
            client
                .community_topic_slice(&epoch, key, Action::Refresh)
                .await
                .unwrap()
        )["replace"],
        true
    );
    client
        .community_topic_slice(&epoch, key, Action::Hot)
        .await
        .unwrap();
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [DETAIL, HOT, ALL, ALL, ALL, ALL, NOTICE, ALL, HOT]
    );
    assert_eq!(calls[0].payload, json!({"actname":TOPIC}));
    assert_eq!(calls[1].payload, json!({"actid":ACT}));
    for (i, cursor) in [
        (2, json!("-1")),
        (3, json!(CURSOR)),
        (4, json!(CURSOR)),
        (5, json!(9007199254740997_u64)),
        (7, json!(-1)),
    ] {
        assert_eq!(calls[i].payload, json!({"actid":ACT,"lasttime":cursor}));
    }
    assert!(
        calls[6].payload["lasttime"]
            .as_u64()
            .is_some_and(|ms| ms > 1700000000000)
    );
    assert!(calls.iter().all(|call| !call.cache));
}

#[tokio::test]
async fn topics_and_accounts_cannot_reuse_another_cursor_or_event_authority() {
    let mut anonymous = row(1, 73);
    anonymous["anonymityInfo"] = json!({"encryptUserId":"HIDDEN"});
    let mut foreign = row(3, 73);
    foreign["actId"] = json!("42");
    let (_dir, api, session, client, epoch) = fixture([
        meta(json!(ACT)).into(),
        events(vec![row(1, 73), foreign], true, json!(CURSOR)).into(),
        json!({"code":200,"events":[anonymous]}).into(),
        meta(json!(42)).into(),
        json!({"code":404}).into(),
    ]);
    let a = data(client.community_topic(&epoch, TOPIC).await.unwrap());
    let akey = field(&a, "handle");
    let first = data(
        client
            .community_topic_slice(&epoch, akey, Action::First)
            .await
            .unwrap(),
    );
    assert_eq!(first["cards"].as_array().unwrap().len(), 1);
    let old = field(&first["cards"][0], "detail");
    let hot = data(
        client
            .community_topic_slice(&epoch, akey, Action::Hot)
            .await
            .unwrap(),
    );
    assert_eq!(hot["cards"][0]["key"], first["cards"][0]["key"]);
    assert_eq!(hot["cards"][0]["authorName"], "匿名用户");
    assert!(hot["cards"][0]["detail"].is_null());
    assert!(matches!(
        client.community_detail(&epoch, old).await,
        Err(Error::StaleResponse)
    ));
    let b = data(client.community_topic(&epoch, "新话题").await.unwrap());
    let bkey = field(&b, "handle");
    assert!(matches!(
        client
            .community_topic_slice(&epoch, bkey, next(&first))
            .await,
        Err(Error::StaleResponse)
    ));
    let unavailable = data(
        client
            .community_topic_slice(&epoch, bkey, Action::First)
            .await
            .unwrap(),
    );
    assert_eq!(unavailable["unavailable"], true);
    assert!(client.community_topic_notice(&epoch, bkey).await.is_err());
    assert!(client.community_topic(&epoch, "恶意/路径").await.is_err());
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let next_epoch = session.token().unwrap().generation.to_string();
    assert!(
        client
            .community_topic_slice(&next_epoch, akey, Action::Hot)
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 5);
    assert_eq!(api.calls()[4].payload, json!({"actid":42,"lasttime":"-1"}));
}

#[tokio::test]
async fn superseded_metadata_can_never_create_a_capability_after_an_account_switch() {
    let (waiting, finish) = gate();
    let (_dir, api, session, client, epoch) = fixture([waiting]);
    let request = tokio::spawn(async move { client.community_topic(&epoch, TOPIC).await });
    api.wait_for_calls(1).await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    finish.send(Ok(meta(json!(ACT)))).unwrap();
    assert!(matches!(request.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls().len(), 1);
}
