mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::ProfilePlayRecordPeriod as Period},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const RECORD: &str = "/api/v1/play/record";
const GUEST: &str = "9007199254740993";
const DETAIL: &str = "/api/w/v1/user/detail/9007199254740993";

fn fixture(
    replies: impl IntoIterator<Item = Reply>,
) -> (
    tempfile::TempDir,
    Arc<ScriptApi>,
    Arc<AccountSession>,
    Arc<MusicClient>,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (dir, api, session, client, epoch)
}

fn detail(visibility: Option<bool>) -> Value {
    let mut response =
        json!({"code":200,"profile":{"userId":GUEST,"nickname":"合成访客"},"listenSongs":27});
    if let Some(visibility) = visibility {
        response["peopleCanSeeMyPlayRecord"] = json!(visibility);
    }
    response
}

fn rows() -> Value {
    json!([{"song":{"id":71,"name":"合成曲目","artists":[{"name":"合成歌手"}],"url":"https://private.invalid/secret","voice_id":22},"playCount":54,"score":99}])
}

#[tokio::test]
async fn exact_week_and_all_requests_preserve_service_order_and_keep_guest_counts_native() {
    let (_dir, api, _, client, epoch) = fixture([
        json!({"code":200,"weekData":rows()}).into(),
        detail(Some(true)).into(),
        json!({"code":"200","weekData":[],"allData":rows()}).into(),
    ]);
    let own = to_value(
        client
            .profile_play_record(&epoch, None, Period::Week)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(own["epoch"], epoch);
    assert_eq!(own["value"]["isSelf"], true);
    assert_eq!(own["value"]["items"][0]["playCount"], 54);
    let guest = to_value(
        client
            .profile_play_record(&epoch, Some(GUEST), Period::All)
            .await
            .unwrap(),
    )
    .unwrap();
    let row = &guest["value"]["items"][0];
    assert_eq!(guest["value"]["isSelf"], false);
    assert_eq!(row["track"]["id"], "71");
    assert_eq!(row["track"]["artists"][0], "合成歌手");
    assert!(row.get("playCount").is_none());
    for forbidden in ["private.invalid", "voiceId", "\"score\""] {
        assert!(!guest.to_string().contains(forbidden));
    }
    let calls = api.calls();
    assert_eq!(calls[0].path, RECORD);
    assert_eq!(
        calls[0].payload,
        json!({"offset":0,"total":true,"limit":120,"uid":"9","type":1})
    );
    assert_eq!(calls[1].path, DETAIL);
    assert_eq!(calls[1].payload, json!({"all":true,"userId":GUEST}));
    assert_eq!(calls[2].path, RECORD);
    assert_eq!(
        calls[2].payload,
        json!({"offset":0,"total":true,"limit":120,"uid":GUEST,"type":0})
    );
    assert!(calls.iter().all(|call| !call.cache));
}

#[tokio::test]
async fn profile_entry_and_fresh_permission_are_distinct_from_an_empty_or_failed_rank() {
    let mut blocked = detail(Some(true));
    blocked["profile"]["inBlacklist"] = json!(true);
    let (_dir, api, _, client, epoch) = fixture([
        detail(Some(true)).into(),
        json!({"code":200,"playlist":[],"more":false}).into(),
        detail(Some(false)).into(),
        detail(None).into(),
        blocked.into(),
        detail(Some(true)).into(),
        json!({"code":"403"}).into(),
        json!({"code":200,"weekData":[]}).into(),
        json!({"code":200,"weekData":null}).into(),
    ]);
    let profile = to_value(
        client
            .playlist_creator_profile(&epoch, GUEST)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        profile["value"]["listeningRank"],
        json!({"isSelf":false,"listenSongs":27})
    );
    for _ in 0..4 {
        let result = client
            .profile_play_record(&epoch, Some(GUEST), Period::Week)
            .await
            .unwrap();
        assert_eq!(
            to_value(result).unwrap()["value"],
            json!({"state":"denied"})
        );
    }
    let empty = client
        .profile_play_record(&epoch, None, Period::Week)
        .await
        .unwrap();
    assert_eq!(
        to_value(empty).unwrap()["value"],
        json!({"state":"ready","isSelf":true,"items":[]})
    );
    assert!(matches!(
        client.profile_play_record(&epoch, None, Period::Week).await,
        Err(Error::Response(_))
    ));
    assert_eq!(
        api.calls()
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        [
            DETAIL,
            "/api/user/playlist",
            DETAIL,
            DETAIL,
            DETAIL,
            DETAIL,
            RECORD,
            RECORD,
            RECORD
        ]
    );
}

#[tokio::test]
async fn previous_account_result_cannot_escape_ipc_after_switch() {
    let (reply, send) = gate();
    let (_dir, api, session, client, epoch) = fixture([reply]);
    let pending =
        tokio::spawn(async move { client.profile_play_record(&epoch, None, Period::Week).await });
    api.wait_for_calls(1).await;
    session
        .adopt(api, json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    send.send(Ok(json!({"code":200,"weekData":rows()})))
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
}
