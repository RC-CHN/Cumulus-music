#[path = "support/resources_recovery.rs"]
mod recovery;
mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession,
        resources::{AccountResources, MutationPhase, ResourceKind},
    },
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

struct World {
    directory: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ScriptApi>,
    session: Arc<AccountSession>,
    resources: Arc<AccountResources>,
}
impl World {
    async fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(directory.path()).unwrap());
        let mut responses = vec![json!({"code":200,"profile":{"userId":1}}).into()];
        responses.extend(replies);
        let api = Arc::new(ScriptApi::new(responses));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let resources = Arc::new(AccountResources::new(session.clone()).unwrap());
        Self {
            directory,
            store,
            api,
            session,
            resources,
        }
    }
}
fn likes(ids: &[&str]) -> Reply {
    json!({"code":200,"ids":ids}).into()
}

#[tokio::test]
async fn confirmed_changes_and_noops_cache_exact_readbacks_and_preserve_existing_order() {
    let world = World::new([
        likes(&["10"]),
        json!({"code":200}).into(),
        likes(&["20", "10"]),
        likes(&["20", "10"]),
        likes(&["20", "10"]),
        json!({"code":200}).into(),
        likes(&["10"]),
    ])
    .await;
    assert_eq!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(
        world.store.get("1", "resources", "liked").unwrap(),
        Some(json!(["20", "10"]))
    );
    assert_eq!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await
            .unwrap()
            .phase,
        MutationPhase::Unchanged
    );
    assert_eq!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), false)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(
        world.store.get("1", "resources", "liked").unwrap(),
        Some(json!(["10"]))
    );
    let calls = world.api.calls();
    let writes: Vec<_> = calls
        .iter()
        .filter(|c| c.path == "/api/song/like")
        .collect();
    assert_eq!(writes.len(), 2);
    assert_eq!(
        writes[0].payload,
        json!({"trackId":"20","userid":"1","like":true})
    );
    assert_eq!(writes[1].payload["like"], false);
}

#[tokio::test]
async fn lost_write_acknowledgement_survives_restart_and_only_reconciles_by_read() {
    let world = World::new([
        likes(&["10"]),
        Reply::Ready(Err(Error::Transport("synthetic lost reply".into()))),
    ])
    .await;
    assert!(matches!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await,
        Err(Error::Transport(_))
    ));
    assert_eq!(
        world.resources.operations().unwrap()["liked:20"].phase,
        MutationPhase::Uncertain
    );
    let store = Arc::new(StateStore::open(world.directory.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into(),
        likes(&["20", "10"]),
    ]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session.refresh().await.unwrap();
    let resources = AccountResources::new(session).unwrap();
    assert!(matches!(
        resources
            .set_membership(ResourceKind::Liked, &json!(20), false)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        resources
            .reconcile(ResourceKind::Liked, "20")
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(api.calls().len(), 2);
    assert!(api.calls().iter().all(|c| c.path != "/api/song/like"));
    assert_eq!(
        store.get("1", "resources", "liked").unwrap(),
        Some(json!(["20", "10"]))
    );
}

#[tokio::test]
async fn accepted_502_without_matching_readback_stays_pending_and_blocks_opposite_write() {
    let world = World::new([
        likes(&["10"]),
        json!({"code":502}).into(),
        likes(&["10"]),
        likes(&["10"]),
    ])
    .await;
    assert!(matches!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        world.resources.reconcile(ResourceKind::Liked, "20").await,
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), false)
            .await,
        Err(Error::MutationUncertain)
    ));
    let record = world
        .resources
        .operations()
        .unwrap()
        .remove("liked:20")
        .unwrap();
    assert_eq!(record.phase, MutationPhase::Uncertain);
    assert_eq!(record.service_code, Some(json!(502)));
    assert_eq!(record.observed, Some(json!(false)));
    assert_eq!(
        world
            .api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/song/like")
            .count(),
        1
    );
}

#[tokio::test]
async fn explicit_rejection_preserves_confirmed_cache_and_error_code() {
    let world = World::new([likes(&["10"]), json!({"code":403}).into()]).await;
    assert!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await
            .err()
            .unwrap()
            .is_service_code(403)
    );
    let record = world
        .resources
        .operations()
        .unwrap()
        .remove("liked:20")
        .unwrap();
    assert_eq!(record.phase, MutationPhase::Rejected);
    assert_eq!(record.service_code, Some(json!(403)));
    assert_eq!(
        world.store.get("1", "resources", "liked").unwrap(),
        Some(json!(["10"]))
    );
    assert_eq!(
        world
            .resources
            .reconcile(ResourceKind::Liked, "20")
            .await
            .unwrap()
            .phase,
        MutationPhase::Rejected
    );
    assert_eq!(world.api.calls().len(), 3);
}

#[tokio::test]
async fn account_switch_keeps_old_pending_responsibility_out_of_the_new_uid() {
    let (pending, release) = gate();
    let world = World::new([likes(&["10"]), pending]).await;
    let work = {
        let resources = world.resources.clone();
        tokio::spawn(async move {
            resources
                .set_membership(ResourceKind::Liked, &json!(20), true)
                .await
        })
    };
    world.api.wait_for_calls(3).await;
    world
        .session
        .attach(Arc::new(ScriptApi::new([
            json!({"code":200,"profile":{"userId":2}}).into(),
        ])))
        .unwrap();
    world.session.refresh().await.unwrap();
    assert!(release.send(Ok(json!({"code":200}))).is_ok());
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        world
            .store
            .get("1", "resource-mutations", "liked:20")
            .unwrap()
            .unwrap()["phase"],
        "uncertain"
    );
    assert!(
        world
            .store
            .values("2", "resource-mutations")
            .unwrap()
            .is_empty()
    );
    assert!(
        world
            .store
            .get("2", "resources", "liked")
            .unwrap()
            .is_none()
    );
    assert!(world.resources.operations().unwrap().is_empty());
}

#[tokio::test]
async fn independent_managers_cannot_write_or_reconcile_a_resource_with_an_operation_in_flight() {
    let (pending, release) = gate();
    let world = World::new([likes(&["10"]), pending, likes(&["20", "10"])]).await;
    let work = {
        let resources = world.resources.clone();
        tokio::spawn(async move {
            resources
                .set_membership(ResourceKind::Liked, &json!(20), true)
                .await
        })
    };
    world.api.wait_for_calls(3).await;
    let other_store = Arc::new(StateStore::open(world.directory.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into()
    ]));
    let other_session = Arc::new(AccountSession::new(api.clone(), Some(other_store)));
    other_session.refresh().await.unwrap();
    let other = AccountResources::new(other_session).unwrap();
    assert!(matches!(
        other
            .set_membership(ResourceKind::Liked, &json!(20), false)
            .await,
        Err(Error::MutationBusy)
    ));
    assert!(matches!(
        other.reconcile(ResourceKind::Liked, "20").await,
        Err(Error::MutationBusy)
    ));
    assert_eq!(api.calls().len(), 1);
    assert!(release.send(Ok(json!({"code":200}))).is_ok());
    assert_eq!(work.await.unwrap().unwrap().phase, MutationPhase::Confirmed);
}

#[tokio::test]
async fn aborted_future_releases_file_lock_but_retains_durable_sending_intent() {
    let (pending, _release) = gate();
    let world = World::new([likes(&["10"]), pending, likes(&["20", "10"])]).await;
    let work = {
        let resources = world.resources.clone();
        tokio::spawn(async move {
            resources
                .set_membership(ResourceKind::Liked, &json!(20), true)
                .await
        })
    };
    world.api.wait_for_calls(3).await;
    work.abort();
    assert!(work.await.is_err());
    assert_eq!(
        world.resources.operations().unwrap()["liked:20"].phase,
        MutationPhase::Sending
    );
    assert!(matches!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), false)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        world
            .resources
            .reconcile(ResourceKind::Liked, "20")
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(
        world
            .api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/song/like")
            .count(),
        1
    );
}

#[tokio::test]
async fn setting_patch_is_narrow_and_unknown_missing_or_wrong_type_state_cannot_be_overwritten() {
    let baseline = json!({"allowLikedNotify":true,"shareSetting":1,"unrelated":"keep"});
    let changed = json!({"allowLikedNotify":false,"shareSetting":1,"unrelated":"keep"});
    let world = World::new([
        json!({"code":200,"setting":baseline}).into(),
        json!({"code":200,"setting":baseline}).into(),
        json!({"code":200}).into(),
        json!({"code":200,"setting":changed}).into(),
        json!({"code":200,"setting":{"shareSetting":1.0}}).into(),
        json!({"code":200,"setting":{"unrelated":"keep"}}).into(),
    ])
    .await;
    assert_eq!(
        world
            .resources
            .set_setting("allowLikedNotify", json!(false))
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(
        world.api.calls()[3].payload,
        json!({"allowLikedNotify":false})
    );
    assert_eq!(
        world.store.get("1", "resources", "settings").unwrap(),
        Some(changed)
    );
    assert!(matches!(
        world.resources.set_setting("shareSetting", json!(0)).await,
        Err(Error::Response(_))
    ));
    assert!(matches!(
        world
            .resources
            .set_setting("allowLikedNotify", json!(true))
            .await,
        Err(Error::Response(_))
    ));
    for (key, value) in [
        ("shareSetting", json!(true)),
        ("personalizedRecommend", json!(false)),
        ("playRecordSetting", json!(3)),
    ] {
        assert!(matches!(
            world.resources.set_setting(key, value).await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(
        world
            .api
            .calls()
            .iter()
            .filter(|c| c.path.ends_with("setting/update"))
            .count(),
        1
    );
}

#[tokio::test]
async fn journal_write_failure_stops_before_remote_mutation() {
    let world = World::new([likes(&["10"])]).await;
    let db = rusqlite::Connection::open(world.directory.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_journal BEFORE INSERT ON state WHEN NEW.kind='resource-mutations' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").unwrap();
    assert!(matches!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await,
        Err(Error::Storage(_))
    ));
    assert_eq!(world.api.calls().len(), 2);
    assert_eq!(
        world.store.get("1", "resources", "liked").unwrap(),
        Some(json!(["10"]))
    );
}

#[tokio::test]
async fn album_and_artist_collection_workflows_read_full_membership_after_writes() {
    let world = World::new([
        json!({"code":200,"data":[],"hasMore":false}).into(),
        json!({"code":200}).into(),
        json!({"code":200,"data":[{"id":30}],"hasMore":false}).into(),
        json!({"code":200,"data":[],"more":false}).into(),
        json!({"code":201}).into(),
        json!({"code":200,"data":[{"id":40}],"more":false}).into(),
    ])
    .await;
    assert_eq!(
        world
            .resources
            .set_membership(ResourceKind::Albums, &json!(30), true)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(
        world
            .resources
            .set_membership(ResourceKind::Artists, &json!(40), true)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(world.api.calls()[2].path, "/api/album/sub");
    assert_eq!(world.api.calls()[5].payload, json!({"artistId":"40"}));
}

#[test]
fn portable_lock_scopes_distinguish_account_and_resource_and_release_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let first = OperationLock::acquire(directory.path(), "1", "liked:20").unwrap();
    assert!(matches!(
        OperationLock::acquire(directory.path(), "1", "liked:20"),
        Err(Error::MutationBusy)
    ));
    let _other_account = OperationLock::acquire(directory.path(), "2", "liked:20").unwrap();
    let _other_resource = OperationLock::acquire(directory.path(), "1", "liked:21").unwrap();
    drop(first);
    assert!(OperationLock::acquire(directory.path(), "1", "liked:20").is_ok());
    let raw:Value=serde_json::from_str(r#"{"kind":"liked","key":"20","before":false,"desired":true,"created_at":1.0,"phase":"sending"}"#).unwrap();
    let record: cumulus_core::account::resources::MutationRecord =
        serde_json::from_value(raw).unwrap();
    assert!(record.phase.pending());
}
