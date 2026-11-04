mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const LIST: &str = "/api/user/playlist";
const PINS: &str = "/api/user/playlist/getTop";
const COUNT: &str = "/api/djradio/get/create/sub/count";

fn profile(uid: u32, blocked: bool) -> Value {
    json!({"code":200,"profile":{"userId":uid,"nickname":"合成作者","inBlacklist":blocked}})
}
fn playlist(id: u32, uid: u32, privacy: u32) -> Value {
    json!({"id":id,"userId":uid,"name":format!("合成歌单 {id}"),"privacy":privacy,
        "playCount":91,"creator":{"userId":uid,"nickname":"合成作者"},"secret":"NOT_PROJECTED"})
}
fn list(rows: Vec<Value>, more: bool) -> Value {
    json!({"code":200,"playlist":rows,"more":more})
}
fn pins(rows: Vec<Value>) -> Value {
    json!({"code":200,"data":{"topItemList":rows}})
}
fn ids(value: &Value, group: &str) -> Vec<String> {
    value[group]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap().into())
        .collect()
}

fn client(
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

#[tokio::test]
async fn owner_sees_explicitly_owned_private_rows_and_pins_preserve_base_order_and_fields() {
    let mut liked = playlist(1, 9, 10);
    liked["specialType"] = json!(5);
    let mut inconsistent = playlist(5, 9, 10);
    inconsistent["creator"]["userId"] = json!(73);
    let mut no_owner = playlist(6, 9, 10);
    no_owner.as_object_mut().unwrap().remove("userId");
    let (_dir, api, _, client, epoch) = client([
        profile(9, false).into(),
        json!({"code":200}).into(),
        list(
            vec![
                liked,
                playlist(2, 9, 0),
                playlist(3, 9, 10),
                playlist(4, 73, 10),
                inconsistent,
                no_owner,
                playlist(10, 73, 0),
                playlist(11, 73, 0),
            ],
            true,
        )
        .into(),
        pins(vec![
            json!({"id":3,"name":"UNTRUSTED PIN"}),
            playlist(11, 73, 0),
            playlist(7, 9, 10),
            playlist(13, 9, 0),
            json!({"id":8}),
            playlist(9, 9, 0),
        ])
        .into(),
        list(
            vec![playlist(8, 9, 10), playlist(9, 9, 10), playlist(12, 9, 0)],
            false,
        )
        .into(),
        json!({"code":200,"data":"malformed optional pins"}).into(),
    ]);
    let value = to_value(client.playlist_creator_profile(&epoch, "9").await.unwrap()).unwrap();
    let view = &value["value"];
    assert_eq!(ids(view, "created"), ["1", "3", "13", "8", "2", "9", "12"]);
    assert_eq!(ids(view, "collected"), ["11", "10"]);
    assert_eq!(view["created"][0]["favorite"], true);
    assert_eq!(view["created"][1]["name"], "合成歌单 3");
    assert_eq!(view["created"][1]["private"], true);
    assert_eq!(view["created"][1]["playCount"], Value::Null);
    assert!(!view.to_string().contains("NOT_PROJECTED"));
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        ["/api/w/v1/user/detail/9", COUNT, LIST, PINS, LIST, PINS]
    );
    for (call, (key, offset)) in
        calls[2..]
            .iter()
            .zip([("uid", 0), ("userId", 0), ("uid", 1000), ("userId", 1000)])
    {
        assert_eq!(call.payload, json!({key:"9","offset":offset,"limit":1000}));
        assert!(!call.cache);
    }
}

#[tokio::test]
async fn guest_never_gets_private_rows_and_optional_pin_failure_does_not_erase_base() {
    let (_dir, api, _, client, epoch) = client([
        profile(73, false).into(),
        json!({"code":200}).into(),
        list(
            vec![
                playlist(20, 73, 0),
                playlist(21, 73, 10),
                playlist(22, 9, 10),
                playlist(23, 9, 0),
                playlist(24, 73, 0),
                playlist(25, 73, 1),
                playlist(26, 73, 0),
            ],
            true,
        )
        .into(),
        pins(vec![
            playlist(21, 73, 0),
            playlist(22, 9, 10),
            playlist(24, 73, 0),
            playlist(27, 73, 0),
            playlist(30, 73, 0),
            playlist(31, 73, 10),
        ])
        .into(),
        list(vec![playlist(30, 73, 10), playlist(32, 73, 0)], false).into(),
        json!({"code":503}).into(),
        profile(74, true).into(),
    ]);
    let view = to_value(client.playlist_creator_profile(&epoch, "73").await.unwrap()).unwrap();
    let view = &view["value"];
    assert_eq!(ids(view, "created"), ["20", "24", "27", "26", "32"]);
    assert_eq!(ids(view, "collected"), ["23"]);
    assert!(!view.to_string().contains("\"private\":true"));
    let blocked = to_value(client.playlist_creator_profile(&epoch, "74").await.unwrap()).unwrap();
    assert_eq!(blocked["value"]["playlistsVisible"], false);
    assert_eq!(blocked["value"]["created"], json!([]));
    assert_eq!(api.calls().last().unwrap().path, "/api/w/v1/user/detail/74");
    assert_eq!(api.calls().len(), 7);
}

#[tokio::test]
async fn switching_accounts_while_base_is_loading_prevents_pin_dispatch_and_projection() {
    let (held, complete) = gate();
    let (_dir, api, session, client, epoch) =
        client([profile(9, false).into(), json!({"code":200}).into(), held]);
    let pending = tokio::spawn(async move { client.playlist_creator_profile(&epoch, "9").await });
    api.wait_for_calls(3).await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    complete
        .send(Ok(list(vec![playlist(1, 9, 10)], false)))
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls().len(), 3);
}
