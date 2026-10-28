use super::*;
fn lost() -> Reply {
    Reply::Ready(Err(Error::Transport(
        "synthetic missing acknowledgement".into(),
    )))
}
fn record(world: &World) -> cumulus_core::account::resources::MutationRecord {
    world
        .resources
        .operations()
        .unwrap()
        .remove("liked:20")
        .unwrap()
}
async fn create(world: &World) {
    assert!(
        world
            .resources
            .set_membership(ResourceKind::Liked, &json!(20), true)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn explicit_acceptance_keeps_original_intent_and_does_not_replay_a_write() {
    let world = World::new([
        likes(&[]),
        lost(),
        likes(&[]),
        likes(&[]),
        likes(&[]),
        likes(&[]),
        json!({"code":200}).into(),
        likes(&["20"]),
    ])
    .await;
    create(&world).await;
    let token = world.session.token().unwrap();
    let checked = world
        .resources
        .recover_like_for(&token, "20", &record(&world).revision().unwrap(), None)
        .await
        .unwrap();
    assert_eq!(checked.phase, MutationPhase::Uncertain);
    let revision = record(&world).revision().unwrap();
    let accepted = world
        .resources
        .recover_like_for(&token, "20", &revision, Some(false))
        .await
        .unwrap();
    assert_eq!(accepted.phase, MutationPhase::AcceptedCurrentState);
    assert_eq!(accepted.desired, json!(true));
    assert_eq!(accepted.observed, Some(json!(false)));
    assert_eq!(
        world
            .api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/song/like")
            .count(),
        1
    );
    let saved = world
        .store
        .get("1", "resource-recovery-history-v1", &revision)
        .unwrap()
        .unwrap();
    assert_eq!(saved["original"]["phase"], "uncertain");
    assert_eq!(saved["resolution"]["phase"], "accepted_current_state");
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
        world
            .store
            .values("1", "resource-recovery-history-v1")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        world
            .api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/song/like")
            .count(),
        2
    );
}
#[tokio::test]
async fn accept_requires_readback_and_rejects_stale_dialog_or_changed_remote_state() {
    let world = World::new([
        likes(&[]),
        lost(),
        likes(&[]),
        likes(&["20"]),
        likes(&["20"]),
    ])
    .await;
    create(&world).await;
    let token = world.session.token().unwrap();
    let old = record(&world).revision().unwrap();
    assert!(matches!(
        world
            .resources
            .recover_like_for(&token, "20", &old, Some(false))
            .await,
        Err(Error::State(_))
    ));
    assert_eq!(world.api.calls().len(), 3);
    world
        .resources
        .recover_like_for(&token, "20", &old, None)
        .await
        .unwrap();
    assert!(matches!(
        world
            .resources
            .recover_like_for(&token, "20", &old, Some(false))
            .await,
        Err(Error::State("like recovery changed"))
    ));
    let latest = record(&world).revision().unwrap();
    assert!(matches!(
        world
            .resources
            .recover_like_for(&token, "20", &latest, Some(false))
            .await,
        Err(Error::State("like recovery changed"))
    ));
    assert_eq!(record(&world).phase, MutationPhase::Uncertain);
    assert!(
        world
            .store
            .values("1", "resource-recovery-history-v1")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        world
            .resources
            .recover_like_for(&token, "20", &latest, None)
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
async fn acceptance_transaction_rolls_back_history_and_resolution_together() {
    let world = World::new([likes(&[]), lost(), likes(&[]), likes(&[]), likes(&[])]).await;
    create(&world).await;
    let token = world.session.token().unwrap();
    world
        .resources
        .recover_like_for(&token, "20", &record(&world).revision().unwrap(), None)
        .await
        .unwrap();
    let revision = record(&world).revision().unwrap();
    let db = rusqlite::Connection::open(world.store.directory().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_recovery BEFORE INSERT ON state WHEN NEW.kind='resource-mutations' BEGIN SELECT RAISE(ABORT,'synthetic disk failure'); END;").unwrap();
    assert!(
        world
            .resources
            .recover_like_for(&token, "20", &revision, Some(false))
            .await
            .is_err()
    );
    assert_eq!(record(&world).phase, MutationPhase::Uncertain);
    assert_eq!(record(&world).revision().unwrap(), revision);
    assert!(
        world
            .store
            .values("1", "resource-recovery-history-v1")
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn acceptance_account_replacement_and_inflight_write_lock_keep_responsibility() {
    let (pending, release) = gate();
    let world = World::new([likes(&[]), lost(), likes(&[]), pending]).await;
    create(&world).await;
    let token = world.session.token().unwrap();
    world
        .resources
        .recover_like_for(&token, "20", &record(&world).revision().unwrap(), None)
        .await
        .unwrap();
    let revision = record(&world).revision().unwrap();
    let lock = OperationLock::acquire(world.store.directory(), "1", "liked:20").unwrap();
    assert!(matches!(
        world
            .resources
            .recover_like_for(&token, "20", &revision, Some(false))
            .await,
        Err(Error::MutationBusy)
    ));
    drop(lock);
    let task = {
        let service = world.resources.clone();
        tokio::spawn(async move {
            service
                .recover_like_for(&token, "20", &revision, Some(false))
                .await
        })
    };
    world.api.wait_for_calls(5).await;
    world
        .session
        .attach(Arc::new(ScriptApi::new([
            json!({"code":200,"profile":{"userId":2}}).into(),
        ])))
        .unwrap();
    world.session.refresh().await.unwrap();
    release.send(Ok(json!({"code":200,"ids":[]}))).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let saved = world
        .store
        .get("1", "resource-mutations", "liked:20")
        .unwrap()
        .unwrap();
    assert_eq!(saved["phase"], "uncertain");
    assert!(
        world
            .store
            .values("1", "resource-recovery-history-v1")
            .unwrap()
            .is_empty()
    );
    assert!(world.resources.operations().unwrap().is_empty());
}
