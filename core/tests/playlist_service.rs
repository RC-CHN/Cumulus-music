mod support;
use cumulus_core::{Error, account::AccountSession, playlist::*, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

fn row(id: u64, name: &str, owner: u64, tracks: &[u64]) -> Value {
    json!({"id":id,"userId":owner,"name":name,"privacy":10,"specialType":0,"description":"","tags":[],"coverImgId":7,"trackIds":tracks.iter().map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}
fn list(rows: &[Value]) -> Reply {
    json!({"code":200,"playlist":rows,"more":false}).into()
}
fn detail(v: &Value) -> Reply {
    json!({"code":200,"playlist":v}).into()
}
fn ok() -> Reply {
    json!({"code":200}).into()
}

#[test]
fn verified_compatibility_routes_stay_in_the_pc_origin() {
    use cumulus_core::protocol::http::{ApiOrigin, WireRoute, select_route};
    for path in [
        "/api/playlist/order/update",
        "/api/playlist/subscribe",
        "/api/playlist/unsubscribe",
    ] {
        assert_eq!(
            select_route(ApiOrigin::Pc, path, &json!({"id":"10"})).unwrap(),
            WireRoute::Weapi
        );
        assert_eq!(
            select_route(ApiOrigin::Interface, path, &json!({"id":"10"})).unwrap(),
            WireRoute::Eapi
        );
    }
    for path in ["/api/playlist/cover/update", "/api/playlist/update/privacy"] {
        assert_eq!(
            select_route(ApiOrigin::Pc, path, &json!({"id":"10"})).unwrap(),
            WireRoute::Eapi
        );
    }
}
fn lost() -> Reply {
    Reply::Ready(Err(Error::Transport("synthetic lost response".into())))
}
struct World {
    dir: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ScriptApi>,
    session: Arc<AccountSession>,
    service: Arc<PlaylistService>,
}
impl World {
    async fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let mut all = vec![json!({"code":200,"profile":{"userId":1}}).into()];
        all.extend(replies);
        let api = Arc::new(ScriptApi::new(all));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let service = Arc::new(PlaylistService::new(session.clone()).unwrap());
        Self {
            dir,
            store,
            api,
            session,
            service,
        }
    }
    async fn revision(&self) -> String {
        self.service
            .detail(&self.session.token().unwrap(), "10")
            .await
            .unwrap()
            .revision
    }
    async fn run(&self, command: PlaylistCommand) -> cumulus_core::Result<PlaylistMutation> {
        self.service
            .execute(&self.session.token().unwrap(), command)
            .await
    }
}
fn edit(revision: String) -> PlaylistCommand {
    PlaylistCommand::Metadata {
        id: "10".into(),
        revision,
        name: "new".into(),
        tags: vec!["电子".into()],
        description: "description".into(),
    }
}
fn batch(name: i64, tags: i64, desc: i64) -> Reply {
    json!({"code":200,"/api/playlist/update/name":{"code":name},"/api/playlist/tags/update":{"code":tags},"/api/playlist/desc/update":{"code":desc}}).into()
}

#[tokio::test]
async fn validation_and_journal_failure_precede_any_remote_write() {
    let w = World::new([list(&[])]).await;
    assert!(matches!(
        w.run(PlaylistCommand::Create {
            name: "#invalid".into(),
            private: true
        })
        .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(w.api.calls().len(), 1);
    let db = rusqlite::Connection::open(w.dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_playlist_journal BEFORE INSERT ON state WHEN NEW.kind='playlist-mutations' BEGIN SELECT RAISE(FAIL,'synthetic disk failure'); END;").unwrap();
    assert!(matches!(
        w.run(PlaylistCommand::Create {
            name: "valid".into(),
            private: true
        })
        .await,
        Err(Error::Storage(_))
    ));
    assert_eq!(w.api.calls().len(), 2);
    assert!(
        w.api
            .calls()
            .iter()
            .all(|c| c.path != "/api/playlist/create")
    );
}

#[tokio::test]
async fn unsent_preparation_failure_is_terminal_and_does_not_block_a_later_intent() {
    let w = World::new([
        list(&[]),
        Reply::Ready(Err(Error::RequestNotSent(Box::new(Error::State(
            "configuration unavailable",
        ))))),
        list(&[]),
        json!({"code":200,"id":"42"}).into(),
        list(&[row(42, "second", 1, &[])]),
    ])
    .await;
    let error = w
        .run(PlaylistCommand::Create {
            name: "first".into(),
            private: true,
        })
        .await
        .err()
        .expect("preparation failure");
    assert!(matches!(error, Error::RequestNotSent(_)));
    let token = w.session.token().unwrap();
    let records = w.service.operations(&token).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].phase, PlaylistPhase::Rejected);
    assert_eq!(records[0].error_class.as_deref(), Some("not_sent"));
    assert_eq!(
        w.run(PlaylistCommand::Create {
            name: "second".into(),
            private: true
        })
        .await
        .unwrap()
        .phase,
        PlaylistPhase::Confirmed
    );
    let reopened = PlaylistService::new(w.session.clone()).unwrap();
    assert!(
        reopened
            .operations(&token)
            .unwrap()
            .iter()
            .all(|r| !r.phase.pending())
    );
}

#[tokio::test]
async fn acknowledged_legacy_empty_update_is_recorded_as_unsupported_not_as_a_successful_clear() {
    let p = row(10, "test", 1, &[1, 2]);
    let w = World::new([detail(&p), detail(&p)]).await;
    let token = w.session.token().unwrap();
    let before = w.service.detail(&token, "10").await.unwrap();
    let command = PlaylistCommand::Tracks {
        id: "10".into(),
        revision: before.revision.clone(),
        change: TrackChange::LegacyReplace,
        ids: vec![],
    };
    assert!(matches!(
        w.run(command.clone()).await,
        Err(Error::InvalidInput(_))
    ));
    let r = PlaylistMutation {
        operation_id: "a".repeat(32),
        command,
        phase: PlaylistPhase::Uncertain,
        before: serde_json::to_value(before).unwrap(),
        observed: None,
        playlist_id: Some("10".into()),
        created_at: 1.0,
        checked_at: None,
        codes: std::collections::BTreeMap::from([("main".into(), 200)]),
        error_class: None,
    };
    w.store
        .put(
            "1",
            "playlist-mutations",
            &r.operation_id,
            &serde_json::to_value(&r).unwrap(),
        )
        .unwrap();
    let r = w.service.reconcile(&token, &r.operation_id).await.unwrap();
    assert_eq!(r.phase, PlaylistPhase::Rejected);
    assert_eq!(
        r.error_class.as_deref(),
        Some("unsupported_replace_contract")
    );
    assert_eq!(r.observed.unwrap()["tracks"], json!(["1", "2"]));
    assert_eq!(w.api.calls().len(), 3);
}

#[tokio::test]
async fn editor_and_mutation_dtos_exclude_owner_and_native_response_fields() {
    use cumulus_core::{
        account::profiles::ProfileRegistry, client::MusicClient, protocol::http::ApiOrigin,
    };
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let mut p = row(10, "test", 1, &[1, 2]);
    p["cookie"] = json!("SECRET");
    p["token"] = json!("SECRET");
    p["creator"] = json!({"userId":1,"token":"SECRET"});
    let api=Arc::new(ScriptApi::new([json!({"code":200,"profile":{"userId":1}}).into(),detail(&p),detail(&p),json!({"code":200,"categories":{"0":"语种"},"sub":[{"name":"华语","category":0,"cookie":"SECRET"}]}).into()]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session, registry);
    let account = client.restore_account().await.unwrap();
    let editor = client.playlist_editor(&account.epoch, "10").await.unwrap();
    let mutation = client
        .mutate_playlist(
            &account.epoch,
            PlaylistCommand::Metadata {
                id: "10".into(),
                revision: editor.value.revision.clone(),
                name: "test".into(),
                tags: vec![],
                description: String::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(mutation.value.phase, PlaylistPhase::Unchanged);
    let tags = client.playlist_tags(&account.epoch).await.unwrap();
    assert_eq!(tags.value[0].category, "语种");
    let serialized =
        serde_json::to_string(&json!({"editor":editor,"mutation":mutation,"tags":tags})).unwrap();
    for forbidden in ["SECRET", "\"owner\"", "\"before\"", "\"command\"", "userId"] {
        assert!(!serialized.contains(forbidden));
    }
    assert!(matches!(
        client
            .mutate_playlist(
                "0",
                PlaylistCommand::Create {
                    name: "test".into(),
                    private: true
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls().len(), 4);
    assert!(
        serde_json::from_value::<PlaylistCommand>(
            json!({"kind":"create","name":"test","private":true,"cookie":"SECRET"})
        )
        .is_err()
    );
}

#[tokio::test]
async fn create_missing_ack_id_is_reconciled_once_using_new_owned_private_entry() {
    let existing = row(5, "same", 1, &[]);
    let created = row(10, "same", 1, &[]);
    let w = World::new([
        list(std::slice::from_ref(&existing)),
        ok(),
        list(&[created, existing]),
    ])
    .await;
    let r = w
        .run(PlaylistCommand::Create {
            name: "same".into(),
            private: true,
        })
        .await
        .unwrap();
    assert_eq!(r.phase, PlaylistPhase::Confirmed);
    assert_eq!(r.playlist_id.as_deref(), Some("10"));
    assert_eq!(
        w.api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/playlist/create")
            .count(),
        1
    );
    assert_eq!(
        w.api.calls()[2].payload,
        json!({"uid":"1","name":"same","privacy":10})
    );
}

#[tokio::test]
async fn ambiguous_creation_survives_reopen_and_blocks_another_create_without_replaying() {
    let a = row(10, "test", 1, &[]);
    let b = row(11, "test", 1, &[]);
    let w = World::new([list(&[]), lost(), list(&[a.clone(), b])]).await;
    let r = w
        .run(PlaylistCommand::Create {
            name: "test".into(),
            private: true,
        })
        .await
        .unwrap();
    assert_eq!(r.phase, PlaylistPhase::Uncertain);
    assert!(r.playlist_id.is_none());
    assert!(matches!(
        w.run(PlaylistCommand::Create {
            name: "different".into(),
            private: true
        })
        .await,
        Err(Error::MutationUncertain)
    ));
    let store = Arc::new(StateStore::open(w.dir.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into(),
        list(&[a]),
    ]));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.refresh().await.unwrap();
    let service = PlaylistService::new(session.clone()).unwrap();
    let done = service
        .reconcile(&session.token().unwrap(), &r.operation_id)
        .await
        .unwrap();
    assert_eq!(done.phase, PlaylistPhase::Confirmed);
    assert_eq!(done.playlist_id.as_deref(), Some("10"));
    assert_eq!(api.calls().len(), 2);
    assert!(api.calls().iter().all(|c| c.path != "/api/playlist/create"));
}

#[tokio::test]
async fn metadata_partial_reply_retains_actual_fields_and_does_not_replay_successful_children() {
    let before = row(10, "old", 1, &[1, 2]);
    let mut after = before.clone();
    after["tags"] = json!(["电子"]);
    after["description"] = json!("description");
    let w = World::new([
        detail(&before),
        detail(&before),
        batch(400, 200, 200),
        detail(&after),
    ])
    .await;
    let r = w.run(edit(w.revision().await)).await.unwrap();
    assert_eq!(r.phase, PlaylistPhase::Partial);
    assert_eq!(r.codes["name"], 400);
    assert_eq!(r.observed.as_ref().unwrap()["name"], "old");
    assert_eq!(r.observed.as_ref().unwrap()["tags"], json!(["电子"]));
    let calls = w.api.calls();
    let batch = calls
        .iter()
        .filter(|c| c.path == "/api/batch")
        .collect::<Vec<_>>();
    assert_eq!(batch.len(), 1);
    let child: Value = serde_json::from_str(
        batch[0].payload["/api/playlist/desc/update"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(child, json!({"id":"10","desc":"description"}));
}

#[tokio::test]
async fn missing_batch_child_and_lagged_readback_stay_pending_until_all_desired_fields_are_observed()
 {
    let before = row(10, "old", 1, &[]);
    let mut after = before.clone();
    after["name"] = json!("new");
    after["tags"] = json!(["电子"]);
    after["description"] = json!("description");
    let w = World::new([
        detail(&before),
        detail(&before),
        json!({"code":200,"/api/playlist/update/name":{"code":200}}).into(),
        detail(&before),
        detail(&after),
    ])
    .await;
    let r = w.run(edit(w.revision().await)).await.unwrap();
    assert_eq!(r.phase, PlaylistPhase::Uncertain);
    assert!(matches!(
        w.run(edit("a".repeat(64))).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        w.service
            .reconcile(&w.session.token().unwrap(), &r.operation_id)
            .await
            .unwrap()
            .phase,
        PlaylistPhase::Confirmed
    );
    assert_eq!(
        w.api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/batch")
            .count(),
        1
    );
}

#[tokio::test]
async fn track_mutations_preserve_existing_order_and_require_complete_reorder() {
    let before = row(10, "test", 1, &[1, 2]);
    let added = row(10, "test", 1, &[3, 1, 2]);
    let ordered = row(10, "test", 1, &[2, 3, 1]);
    let removed = row(10, "test", 1, &[2, 1]);
    let empty = row(10, "test", 1, &[]);
    let w = World::new([
        detail(&before),
        detail(&before),
        ok(),
        detail(&added),
        detail(&added),
        detail(&added),
        ok(),
        detail(&ordered),
        detail(&ordered),
        detail(&ordered),
        ok(),
        detail(&removed),
        detail(&removed),
        detail(&removed),
        ok(),
        detail(&empty),
    ])
    .await;
    for (change, ids) in [
        (TrackChange::Add, vec!["3"]),
        (TrackChange::Reorder, vec!["2", "3", "1"]),
        (TrackChange::Delete, vec!["3"]),
        (TrackChange::Clear, vec![]),
    ] {
        let r = w
            .run(PlaylistCommand::Tracks {
                id: "10".into(),
                revision: w.revision().await,
                change,
                ids: ids.into_iter().map(str::to_owned).collect(),
            })
            .await
            .unwrap();
        assert_eq!(r.phase, PlaylistPhase::Confirmed);
    }
    let calls = w.api.calls();
    let writes = calls
        .iter()
        .filter(|c| c.path == "/api/v1/playlist/manipulate/tracks")
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 4);
    assert_eq!(writes[0].payload["imme"], true);
    assert!(writes[1].payload.get("imme").is_none());
    assert_eq!(writes[3].payload["trackIds"], "[\"2\",\"1\"]");
    assert_eq!(writes[3].payload["op"], "del");
}

#[tokio::test]
async fn duplicates_and_absent_deletes_are_noops_and_known_rejection_is_not_success() {
    let row = row(10, "test", 1, &[1, 2]);
    let w = World::new([
        detail(&row),
        detail(&row),
        detail(&row),
        detail(&row),
        detail(&row),
        detail(&row),
        json!({"code":405}).into(),
        detail(&row),
    ])
    .await;
    for (change, id) in [(TrackChange::Add, "1"), (TrackChange::Delete, "3")] {
        let r = w
            .run(PlaylistCommand::Tracks {
                id: "10".into(),
                revision: w.revision().await,
                change,
                ids: vec![id.into()],
            })
            .await
            .unwrap();
        assert_eq!(r.phase, PlaylistPhase::Unchanged);
    }
    let r = w
        .run(PlaylistCommand::Tracks {
            id: "10".into(),
            revision: w.revision().await,
            change: TrackChange::Add,
            ids: vec!["3".into()],
        })
        .await
        .unwrap();
    assert_eq!(r.phase, PlaylistPhase::Rejected);
    assert_eq!(r.codes["main"], 405);
    assert_eq!(
        w.api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/v1/playlist/manipulate/tracks")
            .count(),
        1
    );
}

#[tokio::test]
async fn stale_revision_foreign_owner_system_playlist_and_local_entries_are_rejected_before_writes()
{
    for variant in 0..4 {
        let mut v = row(10, "test", if variant == 1 { 2 } else { 1 }, &[1]);
        if variant == 2 {
            v["specialType"] = json!(5);
        }
        if variant == 3 {
            v["trackIds"][0]["v"] = json!(-1);
        }
        let w = World::new([detail(&v), detail(&v)]).await;
        let actual = w.revision().await;
        let r = w
            .run(PlaylistCommand::Tracks {
                id: "10".into(),
                revision: if variant == 0 { "f".repeat(64) } else { actual },
                change: TrackChange::Reorder,
                ids: vec!["1".into()],
            })
            .await;
        assert!(r.is_err());
        assert_eq!(w.api.calls().len(), 3);
    }
}

#[tokio::test]
async fn cancellation_leaves_sending_journal_and_process_lock_is_released_for_read_recovery() {
    let (held, _send) = gate();
    let created = row(10, "test", 1, &[]);
    let w = World::new([list(&[]), held, list(&[created])]).await;
    let service = w.service.clone();
    let token = w.session.token().unwrap();
    let task = tokio::spawn(async move {
        service
            .execute(
                &token,
                PlaylistCommand::Create {
                    name: "test".into(),
                    private: true,
                },
            )
            .await
    });
    w.api.wait_for_calls(3).await;
    assert!(matches!(
        w.run(PlaylistCommand::Create {
            name: "other".into(),
            private: true
        })
        .await,
        Err(Error::MutationBusy)
    ));
    task.abort();
    let _ = task.await;
    let record = w
        .service
        .operations(&w.session.token().unwrap())
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.phase, PlaylistPhase::Sending);
    assert_eq!(
        w.service
            .reconcile(&w.session.token().unwrap(), &record.operation_id)
            .await
            .unwrap()
            .phase,
        PlaylistPhase::Confirmed
    );
}

#[tokio::test]
async fn stale_write_retains_original_account_journal_and_cannot_publish_into_new_account() {
    let (held, send) = gate();
    let w = World::new([list(&[]), held]).await;
    let service = w.service.clone();
    let token = w.session.token().unwrap();
    let old = token.clone();
    let task = tokio::spawn(async move {
        service
            .execute(
                &token,
                PlaylistCommand::Create {
                    name: "test".into(),
                    private: true,
                },
            )
            .await
    });
    w.api.wait_for_calls(3).await;
    let other = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":2}}).into()
    ]));
    w.session.attach(other).unwrap();
    w.session.refresh().await.unwrap();
    send.send(Ok(json!({"code":200,"id":10}))).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(w.store.values("2", "playlist-cache").unwrap().is_empty());
    assert_eq!(w.store.values("1", "playlist-mutations").unwrap().len(), 1);
    assert!(matches!(
        w.service.operations(&old),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn order_subscribe_cover_publish_and_delete_read_back_separately() {
    let mut favorite = row(5, "liked", 1, &[]);
    favorite["specialType"] = json!(5);
    let a = row(10, "test", 1, &[]);
    let b = row(11, "other", 1, &[]);
    let mut covered = a.clone();
    covered["coverImgId_str"] = json!("99999999999999999999");
    let mut public = covered.clone();
    public["privacy"] = json!(0);
    let w = World::new([
        list(&[favorite.clone(), a.clone(), b.clone()]),
        list(&[favorite.clone(), a.clone(), b.clone()]),
        ok(),
        list(&[favorite.clone(), b.clone(), a.clone()]),
        detail(&a),
        detail(&a),
        ok(),
        detail(&covered),
        detail(&covered),
        detail(&covered),
        ok(),
        detail(&public),
        detail(&public),
        detail(&public),
        ok(),
        list(&[favorite, b]),
    ])
    .await;
    let token = w.session.token().unwrap();
    let snapshot = w.service.list(&token).await.unwrap();
    assert_eq!(
        w.run(PlaylistCommand::Order {
            group: PlaylistGroup::Created,
            revision: snapshot.revision,
            ids: vec!["5".into(), "11".into(), "10".into()]
        })
        .await
        .unwrap()
        .phase,
        PlaylistPhase::Confirmed
    );
    for cmd in [0, 1, 2] {
        let revision = w.revision().await;
        let action = match cmd {
            0 => PlaylistCommand::Cover {
                id: "10".into(),
                revision,
                document_id: "99999999999999999999".into(),
            },
            1 => PlaylistCommand::Publish {
                id: "10".into(),
                revision,
            },
            _ => PlaylistCommand::Delete {
                id: "10".into(),
                revision,
            },
        };
        assert_eq!(w.run(action).await.unwrap().phase, PlaylistPhase::Confirmed);
    }
    let foreign = row(20, "foreign", 2, &[]);
    let w = World::new([
        list(&[]),
        detail(&foreign),
        ok(),
        list(std::slice::from_ref(&foreign)),
        list(std::slice::from_ref(&foreign)),
        ok(),
        list(&[]),
        list(&[]),
    ])
    .await;
    for subscribed in [true, false, false] {
        let result = w
            .run(PlaylistCommand::Subscribe {
                id: "20".into(),
                subscribed,
            })
            .await
            .unwrap();
        assert!(matches!(
            result.phase,
            PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
        ));
    }
    assert_eq!(
        w.api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/playlist/unsubscribe")
            .count(),
        1
    );
}
