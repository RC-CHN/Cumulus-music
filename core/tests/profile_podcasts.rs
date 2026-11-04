mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::ProfilePodcastQuery as Query},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const DETAIL: &str = "/api/w/v1/user/detail/73";
const COUNT: &str = "/api/djradio/get/create/sub/count";
const CREATED: &str = "/api/djradio/get/byuser/v1";
const COLLECTED: &str = "/api/djradio/get/subed";

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

fn guest(blocked: bool) -> Value {
    json!({"code":200,"profile":{"userId":73,"nickname":"合成播客作者","inBlacklist":blocked}})
}

fn radio(id: u32, owner: u32) -> Value {
    json!({"id":id,"name":format!("合成播客 {id}"),"dj":{"userId":owner,"nickname":"合成播主"},
        "subCount":17,"programCount":3,"subed":true,"rawSecret":"PRIVATE"})
}

#[tokio::test]
async fn header_count_and_both_guest_groups_use_their_exact_independent_contracts() {
    let (_dir, api, _, client, epoch) = fixture([
        guest(false).into(),
        json!({"code":200,"createRadioCount":21,"subRadioCount":2}).into(),
        json!({"code":200,"playlist":[],"more":false}).into(),
        guest(false).into(),
        json!({"code":200,"count":21,"djRadios":[radio(31,73),radio(32,81)]}).into(),
        guest(false).into(),
        json!({"code":200,"count":21,"djRadios":[radio(34,73)]}).into(),
        guest(false).into(),
        json!({"code":200,"count":2,"hasMore":false,"djRadios":[radio(32,81),radio(33,82)]}).into(),
    ]);
    let header = client.playlist_creator_profile(&epoch, "73").await.unwrap();
    assert_eq!(
        to_value(header).unwrap()["value"]["podcasts"],
        json!({"isSelf":false,"total":23})
    );
    let first = client
        .profile_podcasts(&epoch, "73", Query::Created { offset: 0 })
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(first.next_offset, Some(20));
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].card.id, "31");
    let second = client
        .profile_podcasts(&epoch, "73", Query::Created { offset: 20 })
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(second.next_offset, None);
    assert_eq!(second.rows[0].card.id, "34");
    let collected = client
        .profile_podcasts(&epoch, "73", Query::Collected)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(collected.total, 2);
    assert_eq!(collected.rows[0].card.id, "32");
    assert_eq!(collected.rows[0].subscribers, 17);
    assert_eq!(collected.rows[0].card.subscribed, None);
    assert!(!to_value(collected).unwrap().to_string().contains("PRIVATE"));
    let expected = [
        (DETAIL, json!({"all":true,"userId":"73"})),
        (COUNT, json!({"userId":"73"})),
        (
            "/api/user/playlist",
            json!({"uid":"73","offset":0,"limit":1000}),
        ),
        (DETAIL, json!({"all":true,"userId":"73"})),
        (CREATED, json!({"userId":"73","offset":0,"limit":20})),
        (DETAIL, json!({"all":true,"userId":"73"})),
        (CREATED, json!({"userId":"73","offset":20,"limit":20})),
        (DETAIL, json!({"all":true,"userId":"73"})),
        (COLLECTED, json!({"targetUserId":"73","limit":1000})),
    ];
    let actual = api.calls();
    assert_eq!(actual.len(), expected.len());
    for (call, (path, payload)) in actual.iter().zip(expected) {
        assert_eq!(call.path, path);
        assert_eq!(call.payload, payload);
        assert!(!call.cache);
    }
}

#[tokio::test]
async fn fresh_blacklist_blocks_both_guest_groups_and_own_read_uses_authenticated_owner() {
    let (_dir, api, _, client, epoch) = fixture([
        guest(true).into(),
        json!({"code":"403"}).into(),
        json!({"code":200,"count":1001,"hasMore":true,"djRadios":[radio(31,83)]}).into(),
    ]);
    for query in [Query::Created { offset: 0 }, Query::Collected] {
        assert!(
            client
                .profile_podcasts(&epoch, "73", query)
                .await
                .unwrap()
                .value
                .is_none()
        );
    }
    let own = client
        .profile_podcasts(&epoch, "9", Query::Collected)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(own.truncated);
    assert_eq!(own.next_offset, None);
    assert_eq!(own.rows[0].card.id, "31");
    assert!(
        client
            .profile_podcasts(&epoch, "9", Query::Created { offset: 1 })
            .await
            .is_err()
    );
    assert_eq!(
        api.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [DETAIL, DETAIL, COLLECTED]
    );
    assert_eq!(
        api.calls()[2].payload,
        json!({"targetUserId":"9","limit":1000})
    );
}

#[tokio::test]
async fn guest_guard_cannot_dispatch_after_account_switch() {
    let (reply, release) = gate();
    let (_dir, api, session, client, epoch) = fixture([reply]);
    let task = tokio::spawn(async move {
        client
            .profile_podcasts(&epoch, "73", Query::Created { offset: 0 })
            .await
    });
    api.wait_for_calls(1).await;
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    release.send(Ok(guest(false))).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls().len(), 1);
}

#[tokio::test]
async fn owner_browser_handoff_uses_official_short_lived_login_and_plain_url_fallback() {
    let (_dir, api, session, client, epoch) = fixture([
        json!({"token":"synthetic+&?/\u{00a0}token"}).into(),
        json!({"code":503}).into(),
        json!({"token":"expired-synthetic-token"}).into(),
    ]);
    assert!(
        client
            .prepare_profile_podcast_management(&epoch, "73")
            .await
            .is_err()
    );
    assert!(
        client
            .prepare_profile_podcast_management("0", "9")
            .await
            .is_err()
    );
    let prepared = client
        .prepare_profile_podcast_management(&epoch, "9")
        .await
        .unwrap();
    let target = client
        .launch_profile_podcast_management(prepared, |target| Ok(target.to_owned()))
        .unwrap();
    assert_eq!(
        target,
        "https://music.163.com/sso/login?token=synthetic%2B%26%3F%2F%C2%A0token&uid=9&url=%2Fradio%2Fmy%2F%23%2Fplist&forceReplaceLogin=true"
    );
    let fallback = client
        .prepare_profile_podcast_management(&epoch, "9")
        .await
        .unwrap();
    let target = client
        .launch_profile_podcast_management(fallback, |target| Ok(target.to_owned()))
        .unwrap();
    assert_eq!(target, "https://music.163.com/radio/my/#/plist");
    let stale = client
        .prepare_profile_podcast_management(&epoch, "9")
        .await
        .unwrap();
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.launch_profile_podcast_management::<()>(stale, |_| panic!("stale handoff launched")),
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls().len(), 3);
    assert!(
        api.calls()
            .iter()
            .all(|call| call.path == "/api/sso/login/token"
                && call.payload == json!({})
                && !call.cache)
    );
}
