mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::ProfileFollowCommand as Command},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::{path::Path, sync::Arc};
use support::{Reply, ScriptApi, gate};

const TARGET: &str = "73";
const DETAIL: &str = "/api/w/v1/user/detail/73";
const FOLLOW: &str = "/api/user/follow/73";
const UNFOLLOW: &str = "/api/user/delfollow/73";
const PENDING: &str = "profile-follow-pending-v1";

fn make(
    directory: &Path,
    owner: u32,
    replies: impl IntoIterator<Item = Reply>,
) -> (
    Arc<ScriptApi>,
    Arc<AccountSession>,
    Arc<MusicClient>,
    String,
) {
    let registry = ProfileRegistry::open(directory, ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(
            api.clone(),
            json!({"userId":owner}),
            &session.token().unwrap(),
        )
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (api, session, client, epoch)
}

fn detail(followed: bool, mutual: bool, count: u32) -> Value {
    json!({"code":200,"profile":{"userId":73,"nickname":"合成关注目标",
        "followed":followed,"mutual":mutual,"followeds":count}})
}

#[tokio::test]
async fn official_follow_201_unfollow_200_and_readback_project_real_mutual_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    let (api, _, client, epoch) = make(
        dir.path(),
        9,
        [
            detail(false, false, 10).into(),
            json!({"code":201}).into(),
            detail(true, true, 12).into(),
            detail(true, true, 12).into(),
            json!({"code":200}).into(),
            detail(false, true, 8).into(),
        ],
    );
    let first = to_value(
        client
            .profile_follow(&epoch, TARGET, Command::Change { expected: false })
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        first["value"],
        json!({"relation":{"followed":true,"mutual":true,"followersCount":12},
        "pending":false,"status":"followed"})
    );
    let second = to_value(
        client
            .profile_follow(&epoch, TARGET, Command::Change { expected: true })
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        second["value"],
        json!({"relation":{"followed":false,"mutual":false,"followersCount":8},
        "pending":false,"status":"unfollowed"})
    );
    let calls = api.calls();
    assert_eq!(
        calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        [DETAIL, FOLLOW, DETAIL, DETAIL, UNFOLLOW, DETAIL]
    );
    for call in &calls {
        assert!(!call.cache);
        assert_eq!(
            call.payload,
            if call.path == DETAIL {
                json!({"all":true,"userId":"73"})
            } else {
                json!({})
            }
        );
    }
}

#[tokio::test]
async fn only_eligible_guest_intent_can_write_and_known_denial_keeps_authoritative_relation() {
    let dir = tempfile::tempdir().unwrap();
    let mut own_block = detail(false, false, 10);
    own_block["profile"]["blacklist"] = json!(true);
    let mut target_block = detail(false, false, 10);
    target_block["profile"]["inBlacklist"] = json!(true);
    let mut closed = detail(false, false, 10);
    closed["profile"]["accountStatus"] = json!("40");
    let mut wrong = detail(false, false, 10);
    wrong["profile"]["userId"] = json!(999);
    let mut unknown = detail(false, false, 10);
    unknown["profile"]
        .as_object_mut()
        .unwrap()
        .remove("followed");
    let (api, _, client, epoch) = make(
        dir.path(),
        9,
        [
            own_block.into(),
            target_block.into(),
            closed.into(),
            unknown.into(),
            wrong.into(),
            detail(true, true, 11).into(),
            detail(false, false, 10).into(),
            json!({"code":315}).into(),
            detail(false, false, 10).into(),
            json!({"code":320}).into(),
        ],
    );
    let action = Command::Change { expected: false };
    assert!(client.profile_follow(&epoch, "9", action).await.is_err());
    assert!(
        client
            .profile_follow("invalid", TARGET, action)
            .await
            .is_err()
    );
    for _ in 0..4 {
        assert!(
            client
                .profile_follow(&epoch, TARGET, action)
                .await
                .unwrap()
                .value
                .relation
                .is_none()
        );
    }
    assert!(client.profile_follow(&epoch, TARGET, action).await.is_err());
    for status in ["unchanged", "privacy", "limit"] {
        let result =
            to_value(client.profile_follow(&epoch, TARGET, action).await.unwrap()).unwrap();
        assert_eq!(result["value"]["status"], status);
        assert_eq!(result["value"]["pending"], false);
    }
    let calls = api.calls();
    assert_eq!(calls.iter().filter(|c| c.path == FOLLOW).count(), 2);
    assert_eq!(calls.iter().filter(|c| c.path == UNFOLLOW).count(), 0);
}

#[tokio::test]
async fn unknown_send_survives_restart_and_only_readback_can_settle_it() {
    let dir = tempfile::tempdir().unwrap();
    let (api, _, client, epoch) = make(
        dir.path(),
        9,
        [
            detail(false, false, 10).into(),
            Reply::Ready(Err(Error::Transport("synthetic lost reply".into()))),
            detail(false, false, 10).into(),
        ],
    );
    let command = Command::Change { expected: false };
    let value = client
        .profile_follow(&epoch, TARGET, command)
        .await
        .unwrap()
        .value;
    assert!(value.pending);
    assert!(!value.relation.unwrap().followed);
    assert!(matches!(
        client.profile_follow(&epoch, TARGET, command).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        api.calls()
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        [DETAIL, FOLLOW, DETAIL]
    );
    drop(client);
    let (reopened, _, client, epoch) = make(
        dir.path(),
        9,
        [
            detail(false, false, 10).into(),
            json!({"code":200}).into(),
            json!({"code":200,"more":false,"playlist":[]}).into(),
            detail(false, false, 10).into(),
            detail(true, false, 11).into(),
        ],
    );
    assert!(
        client
            .playlist_creator_profile(&epoch, TARGET)
            .await
            .unwrap()
            .value
            .unwrap()
            .follow
            .unwrap()
            .pending
    );
    assert!(
        client
            .profile_follow(&epoch, TARGET, Command::Reconcile)
            .await
            .unwrap()
            .value
            .pending
    );
    let settled = client
        .profile_follow(&epoch, TARGET, Command::Reconcile)
        .await
        .unwrap()
        .value;
    assert!(!settled.pending);
    assert!(settled.relation.unwrap().followed);
    assert!(
        reopened
            .calls()
            .iter()
            .all(|c| c.path != FOLLOW && c.path != UNFOLLOW)
    );
}

#[tokio::test]
async fn old_account_write_receipt_cannot_start_readback_or_expose_pending_to_new_owner() {
    let dir = tempfile::tempdir().unwrap();
    let (held, reply) = gate();
    let (api, session, client, epoch) =
        make(dir.path(), 9, [detail(false, false, 10).into(), held]);
    let action = tokio::spawn(async move {
        client
            .profile_follow(&epoch, TARGET, Command::Change { expected: false })
            .await
    });
    api.wait_for_calls(2).await;
    let next = Arc::new(ScriptApi::new([detail(false, false, 11).into()]));
    session
        .adopt(
            next.clone(),
            json!({"userId":10}),
            &session.token().unwrap(),
        )
        .unwrap();
    reply.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(action.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls().len(), 2);
    let store = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    assert!(store.get("9", PENDING, TARGET).unwrap().is_some());
    assert!(store.get("10", PENDING, TARGET).unwrap().is_none());
}
