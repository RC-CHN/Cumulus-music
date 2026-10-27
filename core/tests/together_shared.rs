mod support;
use cumulus_core::{Error, account::AccountSession, listen_together::*, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};
fn room() -> Value {
    json!({"roomId":"shared-room","creatorId":1,"chatRoomId":"31","agoraChannelId":"41","roomUsers":[{"userId":1},{"userId":2}]})
}
fn status() -> Reply {
    json!({"code":200,"data":{"inRoom":true,"roomInfo":room()}}).into()
}
fn snapshot(version: u64, progress: u64) -> Reply {
    json!({"code":200,"data":{"playlist":{"displayList":{"result":[91,92],"changed":true},"randomList":null,"version":[{"userId":1,"version":version}],"playMode":"ORDER_LOOP","replace":true},"playCommand":{"commandType":"PAUSE","progress":progress,"playStatus":"PAUSE","formerSongId":"91","targetSongId":"91","clientSeq":1,"serverSeq":version,"userId":1}}}).into()
}
fn row(id: u64, playable: bool, reason: &str) -> Value {
    json!({"songId":id,"playable":playable,"unplayableType":reason,"unplayableUserIds":if playable {json!([])} else {json!([2])},"unplayableOuterIds":[],"start":30,"end":60})
}
fn rights() -> Reply {
    json!({"code":200,"data":{"result":[row(92,false,"trial"),row(91,true,"")]}}).into()
}
fn world(path: &std::path::Path, api: Arc<ScriptApi>) -> Arc<AccountSession> {
    let session = Arc::new(AccountSession::new(
        api.clone(),
        Some(Arc::new(StateStore::open(path).unwrap())),
    ));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    session
}

#[tokio::test]
async fn equal_queue_versions_refresh_privileges_and_changed_versions_preserve_ignored_play_commands()
 {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        status(),
        snapshot(1, 10000),
        rights(),
        snapshot(2, 50000),
        rights(),
    ]));
    let session = world(root.path(), api.clone());
    let token = session.token().unwrap();
    let service = TogetherService::new(session).unwrap();
    let initial = service.refresh_for(&token, 1000).await.unwrap();
    let version = initial.snapshot.playlist.unwrap().version;
    let same = service
        .receive_shared_for(&token, "shared-room", Some(&version), true, 2000)
        .await
        .unwrap();
    assert_eq!(api.calls().len(), 3);
    assert!(api.calls()[2].path.ends_with("privilege/get"));
    assert_eq!(same.privileges[0].track_id, "91");
    assert_eq!(
        same.privileges[1].preview,
        Some(Preview {
            start: 30.0,
            end: 60.0
        })
    );
    assert_eq!(same.privileges[1].limited_member("1"), LimitedMember::Other);
    service
        .finish_shared_for(&token, "shared-room", &same.id, IncomingPhase::Applied)
        .unwrap();
    let changed = service
        .receive_shared_for(&token, "shared-room", Some(&[]), true, 3000)
        .await
        .unwrap();
    assert_eq!(
        changed.snapshot.unwrap().play_command.unwrap().progress,
        50000
    );
    let state = service.state_for(&token).unwrap();
    assert_eq!(state.snapshot.play_command.unwrap().progress, 10000);
    assert_eq!(state.playback.latest.unwrap().command.progress, 10000);
    assert_eq!(state.snapshot.playlist.unwrap().version[0].version, 2);
    assert!(matches!(
        service.finish_shared_for(&token, "shared-room", &same.id, IncomingPhase::Applied),
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        api.calls()
            .iter()
            .filter(|c| c.path.ends_with("playlist/get"))
            .count(),
        2
    );
}

#[tokio::test]
async fn interrupted_privilege_read_is_durable_and_recoverable_without_a_remote_write() {
    let root = tempfile::tempdir().unwrap();
    let (pending, _reply) = gate();
    let api = Arc::new(ScriptApi::new([status(), snapshot(1, 10000), pending]));
    let session = world(root.path(), api.clone());
    let token = session.token().unwrap();
    TogetherService::new(session.clone())
        .unwrap()
        .refresh_for(&token, 1000)
        .await
        .unwrap();
    let s = session.clone();
    let t = token.clone();
    let task = tokio::spawn(async move {
        TogetherService::new(s)
            .unwrap()
            .receive_shared_for(
                &t,
                "shared-room",
                Some(&[Version {
                    user_id: "1".into(),
                    version: 1,
                }]),
                true,
                2000,
            )
            .await
    });
    api.wait_for_calls(3).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let stored = TogetherService::new(session)
        .unwrap()
        .state_for(&token)
        .unwrap();
    assert_eq!(stored.shared.unwrap().phase, IncomingPhase::Pending);
    let api = Arc::new(ScriptApi::new([snapshot(2, 22000), rights()]));
    let session = world(root.path(), api.clone());
    let token = session.token().unwrap();
    let service = TogetherService::new(session).unwrap();
    let receipt = service
        .receive_shared_for(&token, "shared-room", None, false, 3000)
        .await
        .unwrap();
    service
        .finish_shared_for(&token, "shared-room", &receipt.id, IncomingPhase::Applied)
        .unwrap();
    assert_eq!(
        service
            .state_for(&token)
            .unwrap()
            .snapshot
            .play_command
            .unwrap()
            .progress,
        22000
    );
    assert!(api.calls().iter().all(|c| c.path.ends_with("/get")));
    assert!(service.operations_for(&token).unwrap().is_empty());
}

#[test]
fn common_privileges_require_exact_coverage_and_valid_preview_bounds() {
    let normalize = |mut v: Value| {
        v["trackId"] = v["songId"].as_u64().unwrap().to_string().into();
        v
    };
    let ids = vec!["91".into(), "92".into()];
    let full = normalize(row(91, true, ""));
    let mut trial = normalize(row(92, false, "trial"));
    let parsed = CommonPrivilege::from_rows(&ids, &[trial.clone(), full.clone()]).unwrap();
    assert!(parsed.iter().all(CommonPrivilege::can_play));
    assert!(CommonPrivilege::from_rows(&ids, std::slice::from_ref(&full)).is_err());
    assert!(CommonPrivilege::from_rows(&ids, &[full.clone(), full.clone()]).is_err());
    trial["end"] = json!(10);
    assert!(CommonPrivilege::from_rows(&ids, &[full.clone(), trial]).is_err());
    let denied = normalize(row(92, false, "digital_album"));
    let parsed = CommonPrivilege::from_rows(&ids, &[full, denied]).unwrap();
    assert!(!parsed[1].can_play());
}

#[test]
fn common_playable_and_limited_member_match_executed_official_helpers() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/together-common-official.json")).unwrap();
    assert_eq!(oracle["cases"].as_array().unwrap().len(), 10);
    for case in oracle["cases"].as_array().unwrap() {
        let parsed = CommonPrivilege::from_rows(&["91".into()], &[case["row"].clone()]).unwrap();
        assert_eq!(parsed[0].can_play(), case["canPlay"].as_bool().unwrap());
        assert_eq!(json!(parsed[0].limited_member("1")), case["limitedMember"]);
    }
}
