use super::*;
use cumulus_core::playlist::{
    PlaylistMixedAddTarget, PlaylistMixedOrderRow, PlaylistMixedReorderEvidence, PlaylistPhase,
};
use cumulus_core::protocol::api::{
    GuardedPlaylistAdd, GuardedPlaylistRemove, PlaylistAddPermission,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Notify;

const UPDATE: &str = "/api/v1/playlist/manipulate/tracks";

#[derive(Clone, Copy)]
enum Reply {
    Ack,
    AckNotApplied,
    Lost,
    Reject,
    NotSent,
    MissingCode,
    AckAtEnd,
}

struct ReorderServer {
    detail: Mutex<Value>,
    writes: Mutex<Vec<Value>>,
    details: AtomicUsize,
    collect_privilege: Mutex<Option<i64>>,
    reply: Reply,
    scope: Mutex<String>,
    rotate_scope_at_dispatch: AtomicBool,
    hold: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl ReorderServer {
    fn new(ids: &[u32], reply: Reply) -> Self {
        let mut detail = Server::new().detail.into_inner().unwrap();
        detail["trackIds"] = json!(
            ids.iter()
                .map(|id| json!({"id":id,"v":0}))
                .collect::<Vec<_>>()
        );
        Self {
            detail: Mutex::new(detail),
            writes: Mutex::new(Vec::new()),
            details: AtomicUsize::new(0),
            collect_privilege: Mutex::new(None),
            reply,
            scope: Mutex::new("fixture-a".into()),
            rotate_scope_at_dispatch: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        }
    }

    fn current(&self, session: &Arc<AccountSession>) -> PlaylistLocalMembership {
        PlaylistLocalStore::new(session.clone())
            .unwrap()
            .project(
                &session.token().unwrap(),
                "73",
                &self.detail.lock().unwrap(),
            )
            .unwrap()
    }
}

impl Api for ReorderServer {
    fn local_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.scope.lock().unwrap().clone()))
    }

    fn mixed_playlist_add_scope_revision(&self) -> Result<Option<String>> {
        self.local_collection_scope_revision()
    }

    fn request_guarded_playlist_remove<'a>(
        &'a self,
        write: GuardedPlaylistRemove,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(revision, self.scope.lock().unwrap().as_str());
            if self.rotate_scope_at_dispatch.swap(false, Ordering::SeqCst) {
                *self.scope.lock().unwrap() = "fixture-b".into();
            }
            let (path, payload) = write.into_request()?;
            permission()?;
            dispatched.store(true, Ordering::Release);
            self.request(path, payload, false).await
        })
    }

    fn request_guarded_mixed_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        revision: &'a str,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(revision, self.scope.lock().unwrap().as_str());
            if self.rotate_scope_at_dispatch.swap(false, Ordering::SeqCst) {
                *self.scope.lock().unwrap() = "fixture-b".into();
            }
            let (path, payload) = write.into_request()?;
            permission()?;
            dispatched.store(true, Ordering::Release);
            self.request(path, payload, false).await
        })
    }

    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            match path {
                "/api/v6/playlist/detail" => {
                    assert_eq!(payload, json!({"id":"73","n":100000,"s":0}));
                    self.details.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"code":200,"playlist":self.detail.lock().unwrap().clone()}))
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    assert_eq!(ids.len(), 1);
                    let id = ids[0]["id"].as_str().unwrap();
                    let privileges = self
                        .collect_privilege
                        .lock()
                        .unwrap()
                        .map_or_else(Vec::new, |privilege| {
                            vec![json!({"id":id,"subp":privilege})]
                        });
                    Ok(
                        json!({"code":200,"songs":[{"id":id,"name":"Synthetic catalogue song"}],"privileges":privileges}),
                    )
                }
                UPDATE => {
                    assert_eq!(payload["pid"], "73");
                    let op = payload["op"].as_str().unwrap().to_owned();
                    assert!(matches!(op.as_str(), "update" | "del" | "add"));
                    assert!(payload.get("imme").is_none());
                    let ids: Vec<String> =
                        serde_json::from_str(payload["trackIds"].as_str().unwrap()).unwrap();
                    self.writes.lock().unwrap().push(payload);
                    if self.hold.swap(false, Ordering::SeqCst) {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    if matches!(
                        self.reply,
                        Reply::Ack | Reply::Lost | Reply::MissingCode | Reply::AckAtEnd
                    ) {
                        let mut detail = self.detail.lock().unwrap();
                        let original = detail["trackIds"].as_array().unwrap();
                        detail["trackIds"] = if op == "update" {
                            json!(
                                ids.iter()
                                    .map(|id| {
                                        original
                                            .iter()
                                            .find(|row| {
                                                row["id"].to_string().trim_matches('"') == id
                                            })
                                            .expect("update leaked local ID or changed membership")
                                            .clone()
                                    })
                                    .collect::<Vec<_>>()
                            )
                        } else if op == "add" {
                            let added = ids.iter().map(|id| json!({"id":id,"v":0}));
                            if matches!(self.reply, Reply::AckAtEnd) {
                                json!(original.iter().cloned().chain(added).collect::<Vec<_>>())
                            } else {
                                json!(added.chain(original.iter().cloned()).collect::<Vec<_>>())
                            }
                        } else {
                            assert!(
                                ids.iter().all(|id| original
                                    .iter()
                                    .any(|row| { row["id"].to_string().trim_matches('"') == id })),
                                "delete leaked a device ID or an unrecognized online selection"
                            );
                            json!(
                                original
                                    .iter()
                                    .filter(|row| {
                                        !ids.iter()
                                            .any(|id| row["id"].to_string().trim_matches('"') == id)
                                    })
                                    .collect::<Vec<_>>()
                            )
                        };
                    }
                    match self.reply {
                        Reply::Ack | Reply::AckNotApplied | Reply::AckAtEnd => {
                            Ok(json!({"code":"200"}))
                        }
                        Reply::Lost => Err(Error::Transport("synthetic lost response".into())),
                        Reply::Reject => Ok(json!({"code":405, "msg":"synthetic"})),
                        Reply::NotSent => Err(Error::RequestNotSent(Box::new(Error::Cancelled))),
                        Reply::MissingCode => Ok(json!({"msg":"not an explicit code"})),
                    }
                }
                _ => panic!("unexpected playlist call {path}"),
            }
        })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    profile: PathBuf,
    api: Arc<ReorderServer>,
    rows: Vec<PlaylistLocalRef>,
}

impl Fixture {
    async fn new(ids: &[u32], reply: Reply) -> Self {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("profile");
        let media = root.path().join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(media.join("a.wav"), b"synthetic a").unwrap();
        std::fs::write(media.join("b.wav"), b"synthetic b").unwrap();
        let api = Arc::new(ReorderServer::new(ids, reply));
        let (client, session, library) = open(&profile, api.clone(), Some(9));
        library.scan(&media).unwrap();
        let rows = refs(&client).await;
        let before = api.current(&session);
        client
            .mutate_playlist_local("1", "73", &before.revision, add(&rows))
            .await
            .unwrap();
        Self {
            _root: root,
            profile,
            api,
            rows,
        }
    }

    fn open(&self) -> (MusicClient, Arc<AccountSession>) {
        let (client, session, _) = open(&self.profile, self.api.clone(), Some(9));
        (client, session)
    }

    fn local(&self, index: usize) -> PlaylistMixedOrderRow {
        PlaylistMixedOrderRow::Local(self.rows[index].clone())
    }
}

fn online(id: u32) -> PlaylistMixedOrderRow {
    PlaylistMixedOrderRow::Online { id: id.to_string() }
}

async fn add_target(client: &MusicClient, song: &str) -> (String, String) {
    let Some(PlaylistMixedAddTarget::Ready {
        revision,
        operation_id,
    }) = client
        .playlist_mixed_add_target("1", "73", song)
        .await
        .unwrap()
        .value
    else {
        panic!("expected fresh verified mixed destination");
    };
    (revision, operation_id)
}

#[tokio::test]
async fn mixed_add_prepends_one_catalog_song_and_preserves_same_spelled_device_reference() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, session) = fixture.open();
    let collision = "9".repeat(64);
    let reference = PlaylistLocalRef {
        id: collision.clone(),
        revision: "b".repeat(64),
    };
    ProfileRegistry::open(&fixture.profile, ApiOrigin::Pc)
        .unwrap()
        .store()
        .put(
            "9",
            "playlist-local-membership-v1",
            "73",
            &json!({"generation":8,"local":[{"position":1,"reference":reference}]}),
        )
        .unwrap();
    let (revision, operation) = add_target(&client, &collision).await;
    let value = client
        .add_catalog_song_to_mixed_playlist("1", "73", &revision, &collision, &operation)
        .await
        .unwrap()
        .value;
    assert_eq!(value.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        value.evidence,
        Some(PlaylistMixedReorderEvidence::Acknowledged)
    );
    assert_eq!(value.operation_id, operation);
    assert_eq!(
        &*fixture.api.writes.lock().unwrap(),
        &[json!({"pid":"73","trackIds":json!([collision]).to_string(),"op":"add"}),]
    );
    let projected = fixture.api.current(&session);
    assert_eq!(projected.generation(), Some(9));
    assert_eq!(
        projected.rows,
        [
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Remote { index: 1 },
            PlaylistLocalRow::Local(reference),
            PlaylistLocalRow::Remote { index: 2 },
        ]
    );
}

#[tokio::test]
async fn mixed_add_lost_ack_recovers_after_reopen_using_exact_readback_only() {
    let fixture = Fixture::new(&[10, 20], Reply::Lost).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let (revision, operation) = add_target(&client, "30").await;
    let value = client
        .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &operation)
        .await
        .unwrap()
        .value;
    assert_eq!(value.phase, PlaylistPhase::Uncertain);
    assert_eq!(
        before.generation(),
        fixture.api.current(&session).generation()
    );
    drop(client);
    let (client, session) = fixture.open();
    let reads = fixture.api.details.load(Ordering::SeqCst);
    let Some(PlaylistMixedAddTarget::Pending {
        receipt,
        current_selection: true,
    }) = client
        .playlist_mixed_add_target("1", "73", "30")
        .await
        .unwrap()
        .value
    else {
        panic!("expected saved pending operation");
    };
    assert_eq!(receipt, value);
    assert!(matches!(
        client.playlist_mixed_add_target("1", "73", "40").await.unwrap().value,
        Some(PlaylistMixedAddTarget::Pending { receipt: ref earlier, current_selection: false })
            if earlier == &value
    ));
    assert_eq!(fixture.api.details.load(Ordering::SeqCst), reads);
    let shown = fixture.api.current(&session);
    assert!(matches!(
        client
            .remove_playlist_mixed("1", "73", &shown.revision, vec![online(10)])
            .await,
        Err(Error::MutationUncertain),
    ));
    let (other, _, _) = open(&fixture.profile, fixture.api.clone(), Some(99));
    assert!(
        other
            .playlist_mixed_add_target("1", "73", "30")
            .await
            .unwrap()
            .value
            .is_none()
    );
    let confirmed = client
        .reconcile_playlist_mixed_add("1", "73", &operation)
        .await
        .unwrap()
        .value;
    assert_eq!(confirmed.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        confirmed.evidence,
        Some(PlaylistMixedReorderEvidence::ChangedReadback)
    );
    assert_eq!(fixture.api.details.load(Ordering::SeqCst), reads + 1);
    assert_eq!(fixture.api.writes.lock().unwrap().len(), 1);
    assert!(matches!(
        fixture.api.current(&session).rows.as_slice(),
        [
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Local(_),
            PlaylistLocalRow::Local(_),
            PlaylistLocalRow::Remote { index: 1 },
            PlaylistLocalRow::Remote { index: 2 }
        ],
    ));
}

#[tokio::test]
async fn mixed_add_wrong_server_placement_never_commits_local_order_even_with_ack() {
    let fixture = Fixture::new(&[10, 20], Reply::AckAtEnd).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let (revision, operation) = add_target(&client, "30").await;
    let value = client
        .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &operation)
        .await
        .unwrap()
        .value;
    assert_eq!(value.phase, PlaylistPhase::Uncertain);
    assert_eq!(
        before.generation(),
        fixture.api.current(&session).generation()
    );
    assert_eq!(
        client
            .reconcile_playlist_mixed_add("1", "73", &operation)
            .await
            .unwrap()
            .value,
        value,
    );
    assert_eq!(fixture.api.writes.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn mixed_add_blocks_stale_duplicate_denied_and_queue_context_and_fences_credentials() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, session) = fixture.open();
    let (revision, operation) = add_target(&client, "30").await;
    assert!(matches!(
        client
            .add_catalog_song_to_mixed_playlist("1", "73", &"b".repeat(64), "30", &operation)
            .await,
        Err(Error::QueueChanged),
    ));
    assert!(matches!(
        client
            .add_catalog_song_to_mixed_playlist("1", "73", &revision, "10", &operation)
            .await,
        Err(Error::InvalidInput(
            "mixed playlist addition requires new catalog songs"
        )),
    ));
    *fixture.api.collect_privilege.lock().unwrap() = Some(-1);
    assert!(matches!(
        client
            .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &operation)
            .await,
        Err(Error::InvalidInput("catalog song cannot be collected")),
    ));
    *fixture.api.collect_privilege.lock().unwrap() = Some(1);
    let state = ProfileRegistry::open(&fixture.profile, ApiOrigin::Pc)
        .unwrap()
        .store();
    state
        .put(
            "9",
            "playback",
            "queue",
            &json!({
                "items":[{"entry_id":"synthetic-entry","resource":{"id":"30","song":{"id":30}}}],
                "order":["synthetic-entry"],"current":"synthetic-entry",
            }),
        )
        .unwrap();
    assert!(matches!(
        client
            .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &operation)
            .await,
        Err(Error::InvalidInput(
            "queued song requires its original collection context"
        )),
    ));
    state.put("9", "playback", "queue", &json!({})).unwrap();
    assert!(fixture.api.writes.lock().unwrap().is_empty());
    assert!(
        client
            .playlist_mixed_add_receipt("1", "73")
            .unwrap()
            .value
            .is_none()
    );
    fixture
        .api
        .rotate_scope_at_dispatch
        .store(true, Ordering::SeqCst);
    let value = client
        .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &operation)
        .await
        .unwrap()
        .value;
    assert_eq!(value.phase, PlaylistPhase::Rejected);
    assert_eq!(value.evidence, Some(PlaylistMixedReorderEvidence::NotSent));
    assert_eq!(fixture.api.current(&session).revision, revision);
    assert!(fixture.api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn mixed_add_refuses_truncated_detail_and_cannot_replay_an_older_terminal_operation() {
    let fixture = Fixture::new(&[10, 20], Reply::Reject).await;
    let (client, _) = fixture.open();
    fixture.api.detail.lock().unwrap()["trackCount"] = json!(3);
    assert!(matches!(
        client.playlist_mixed_add_target("1", "73", "30").await,
        Err(Error::Response("incomplete mixed playlist track list")),
    ));
    fixture.api.detail.lock().unwrap()["trackCount"] = json!(2);
    // A falsey single-row privilege (raw numeric zero) remains admissible.
    *fixture.api.collect_privilege.lock().unwrap() = Some(0);
    let (revision, first) = add_target(&client, "30").await;
    let rejection = client
        .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &first)
        .await
        .unwrap()
        .value;
    assert_eq!(
        rejection.evidence,
        Some(PlaylistMixedReorderEvidence::ExplicitRejection)
    );
    let (same_revision, second) = add_target(&client, "40").await;
    assert_eq!(same_revision, revision);
    let replacement = client
        .add_catalog_song_to_mixed_playlist("1", "73", &revision, "40", &second)
        .await
        .unwrap()
        .value;
    assert_eq!(replacement.phase, PlaylistPhase::Rejected);
    assert!(matches!(
        client
            .add_catalog_song_to_mixed_playlist("1", "73", &revision, "30", &first)
            .await,
        Err(Error::StaleResponse),
    ));
    assert_eq!(fixture.api.writes.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn mixed_delete_sends_only_selected_online_ids_in_order_and_retains_colliding_remote() {
    let fixture = Fixture::new(&[10, 20, 30], Reply::Ack).await;
    let (client, session) = fixture.open();
    let collision = "9".repeat(64);
    let local_collision = PlaylistLocalRef {
        id: collision.clone(),
        revision: "b".repeat(64),
    };
    // Synthetic stored binding, using a valid all-decimal digest to exercise
    // the exact collision of the independent device and catalog namespaces.
    ProfileRegistry::open(&fixture.profile, ApiOrigin::Pc)
        .unwrap()
        .store()
        .put(
            "9",
            "playlist-local-membership-v1",
            "73",
            &json!({"generation":8,"local":[
                {"position":1,"reference":fixture.rows[0]},
                {"position":3,"reference":local_collision}
            ]}),
        )
        .unwrap();
    fixture.api.detail.lock().unwrap()["trackIds"] =
        json!([{"id":10,"v":0},{"id":collision,"v":0},{"id":20,"v":0},{"id":30,"v":0}]);
    let before = fixture.api.current(&session);
    let receipt = client
        .remove_playlist_mixed(
            "1",
            "73",
            &before.revision,
            vec![
                online(30),
                PlaylistMixedOrderRow::Local(local_collision),
                online(10),
            ],
        )
        .await
        .unwrap()
        .value;
    assert_eq!(receipt.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        receipt.evidence,
        Some(PlaylistMixedReorderEvidence::Acknowledged)
    );
    assert_eq!(
        *fixture.api.writes.lock().unwrap(),
        [json!({
            "pid":"73", "op":"del", "trackIds":"[\"30\",\"10\"]"
        })]
    );
    assert_eq!(
        fixture.api.detail.lock().unwrap()["trackIds"],
        json!([{"id":collision,"v":0},{"id":20,"v":0}])
    );
    let after = fixture.api.current(&session);
    assert_eq!(
        after.rows,
        [
            PlaylistLocalRow::Local(fixture.rows[0].clone()),
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Remote { index: 1 }
        ]
    );
    assert_eq!(after.generation(), Some(9));
    assert!(
        client
            .playlist_mixed_reorder_receipt("1", "73")
            .unwrap()
            .value
            .is_none()
    );
}

#[tokio::test]
async fn mixed_delete_readback_is_send_once_and_can_confirm_last_device_row_after_restart() {
    for reply in [Reply::Lost, Reply::AckNotApplied] {
        let fixture = Fixture::new(&[10, 20], reply).await;
        let (client, session) = fixture.open();
        let before = fixture.api.current(&session);
        let receipt = client
            .remove_playlist_mixed(
                "1",
                "73",
                &before.revision,
                vec![fixture.local(0), online(20), fixture.local(1), online(10)],
            )
            .await
            .unwrap()
            .value;
        assert_eq!(receipt.phase, PlaylistPhase::Uncertain);
        let pending = fixture.api.current(&session);
        assert_eq!(pending.generation(), before.generation());
        assert!(
            pending
                .rows
                .contains(&PlaylistLocalRow::Local(fixture.rows[0].clone()))
        );
        assert!(matches!(
            client
                .move_playlist_mixed("1", "73", &pending.revision, vec![fixture.local(0)], 0)
                .await,
            Err(Error::MutationUncertain)
        ));
        assert!(matches!(
            client
                .mutate_playlist_local("1", "73", &pending.revision, remove(&fixture.rows))
                .await,
            Err(Error::MutationUncertain)
        ));
        drop(client);
        drop(session);

        let (client, session) = fixture.open();
        assert_eq!(
            client
                .playlist_mixed_remove_receipt("1", "73")
                .unwrap()
                .value,
            Some(receipt.clone())
        );
        let (other, _, _) = open(&fixture.profile, fixture.api.clone(), Some(10));
        assert!(matches!(
            other
                .reconcile_playlist_mixed_remove("1", "73", &receipt.operation_id)
                .await,
            Err(Error::StaleResponse)
        ));
        fixture.api.detail.lock().unwrap()["trackIds"] = json!([{"id":30,"v":0}]);
        assert_eq!(
            client
                .reconcile_playlist_mixed_remove("1", "73", &receipt.operation_id)
                .await
                .unwrap()
                .value
                .phase,
            PlaylistPhase::Uncertain
        );
        fixture.api.detail.lock().unwrap()["trackIds"] = json!([]);
        let settled = client
            .reconcile_playlist_mixed_remove("1", "73", &receipt.operation_id)
            .await
            .unwrap()
            .value;
        assert_eq!(settled.phase, PlaylistPhase::Confirmed);
        assert_eq!(
            settled.evidence,
            Some(PlaylistMixedReorderEvidence::ChangedReadback)
        );
        let after = fixture.api.current(&session);
        assert!(after.rows.is_empty());
        assert_eq!(
            after.generation(),
            before.generation().map(|version| version + 1)
        );
        assert_eq!(fixture.api.writes.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn mixed_delete_rejects_ambiguous_detail_and_stale_device_refs_without_any_write() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let mut stale = fixture.rows[0].clone();
    stale.revision = "e".repeat(64);
    for selection in [
        vec![fixture.local(0)],
        vec![online(10), online(10)],
        vec![online(10), PlaylistMixedOrderRow::Local(stale)],
    ] {
        assert!(matches!(
            client
                .remove_playlist_mixed("1", "73", &before.revision, selection)
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    fixture.api.detail.lock().unwrap()["creator"] = json!({"userId":10});
    assert!(matches!(
        client
            .remove_playlist_mixed("1", "73", &before.revision, vec![online(10)])
            .await,
        Err(Error::InvalidInput(_))
    ));
    fixture.api.detail.lock().unwrap()["creator"] = json!({"userId":9});
    fixture.api.detail.lock().unwrap()["trackIds"][0]["v"] = json!(-1);
    let unbound = fixture.api.current(&session);
    assert!(matches!(
        client
            .remove_playlist_mixed("1", "73", &unbound.revision, vec![online(20)])
            .await,
        Err(Error::State(_))
    ));
    assert!(fixture.api.writes.lock().unwrap().is_empty());
    assert!(
        client
            .playlist_mixed_remove_receipt("1", "73")
            .unwrap()
            .value
            .is_none()
    );
}

#[tokio::test]
async fn mixed_delete_refuses_a_credential_change_at_the_actual_dispatch_boundary() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    fixture
        .api
        .rotate_scope_at_dispatch
        .store(true, Ordering::SeqCst);
    let receipt = client
        .remove_playlist_mixed("1", "73", &before.revision, vec![online(10)])
        .await
        .unwrap()
        .value;
    assert_eq!(receipt.phase, PlaylistPhase::Rejected);
    assert_eq!(
        receipt.evidence,
        Some(PlaylistMixedReorderEvidence::NotSent)
    );
    assert!(fixture.api.writes.lock().unwrap().is_empty());
    assert_eq!(fixture.api.current(&session), before);
}

#[tokio::test]
async fn mixed_move_uses_raw_pre_removal_gap_and_selection_order_on_the_full_list() {
    let fixture = Fixture::new(&[10, 20, 30], Reply::Ack).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let same_online = client
        .move_playlist_mixed(
            "1",
            "73",
            &before.revision,
            vec![fixture.local(1), fixture.local(0)],
            3,
        )
        .await
        .unwrap()
        .value;
    assert_eq!(same_online.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        same_online.evidence,
        Some(PlaylistMixedReorderEvidence::Acknowledged)
    );
    let after_first = fixture.api.current(&session);
    assert_eq!(
        after_first.rows,
        [
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Local(fixture.rows[1].clone()),
            PlaylistLocalRow::Local(fixture.rows[0].clone()),
            PlaylistLocalRow::Remote { index: 1 },
            PlaylistLocalRow::Remote { index: 2 },
        ]
    );
    let changed_online = client
        .move_playlist_mixed(
            "1",
            "73",
            &after_first.revision,
            vec![online(30), fixture.local(0), online(10)],
            4,
        )
        .await
        .unwrap()
        .value;
    assert_eq!(changed_online.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        fixture.api.current(&session).rows,
        [
            PlaylistLocalRow::Local(fixture.rows[1].clone()),
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Remote { index: 1 },
            PlaylistLocalRow::Local(fixture.rows[0].clone()),
            PlaylistLocalRow::Remote { index: 2 },
        ]
    );
    assert_eq!(
        *fixture.api.writes.lock().unwrap(),
        [
            json!({"pid":"73","op":"update","trackIds":"[\"10\",\"20\",\"30\"]"}),
            json!({"pid":"73","op":"update","trackIds":"[\"20\",\"30\",\"10\"]"}),
        ]
    );
}

#[tokio::test]
async fn mixed_move_rejects_stale_or_forged_selection_without_sending() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let mut stale_local = fixture.rows[0].clone();
    stale_local.revision = "a".repeat(64);
    for (selected, gap) in [
        (vec![], 0),
        (vec![online(10)], 5),
        (vec![online(10), online(10)], 1),
        (vec![online(99)], 2),
        (vec![PlaylistMixedOrderRow::Local(stale_local)], 1),
    ] {
        assert!(matches!(
            client
                .move_playlist_mixed("1", "73", &before.revision, selected, gap)
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    fixture.api.detail.lock().unwrap()["trackIds"][0]["v"] = json!(1);
    assert!(matches!(
        client
            .move_playlist_mixed("1", "73", &before.revision, vec![online(10)], 1)
            .await,
        Err(Error::QueueChanged)
    ));
    fixture.api.detail.lock().unwrap()["trackIds"][0]["v"] = json!(-1);
    let with_unbound = fixture.api.current(&session);
    assert!(matches!(
        client
            .move_playlist_mixed("1", "73", &with_unbound.revision, vec![fixture.local(0)], 0)
            .await,
        Err(Error::State(
            "cannot reorder unbound server local playlist rows"
        ))
    ));
    assert!(fixture.api.writes.lock().unwrap().is_empty());
    assert!(
        client
            .playlist_mixed_reorder_receipt("1", "73")
            .unwrap()
            .value
            .is_none()
    );
}

#[tokio::test]
async fn mixed_reorder_sends_empty_projection_and_waits_for_explicit_acknowledgement() {
    let fixture = Fixture::new(&[], Reply::Ack).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let client = Arc::new(client);
    fixture.api.hold.store(true, Ordering::SeqCst);
    let pending = tokio::spawn({
        let client = client.clone();
        let revision = before.revision.clone();
        let desired = serde_json::from_value(json!([
            {"kind":"local","id":fixture.rows[1].id,"revision":fixture.rows[1].revision},
            {"kind":"local","id":fixture.rows[0].id,"revision":fixture.rows[0].revision}
        ]))
        .unwrap();
        async move {
            client
                .reorder_playlist_mixed("1", "73", &revision, desired)
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(3), fixture.api.entered.notified())
        .await
        .unwrap();
    let journal = client
        .playlist_mixed_reorder_receipt("1", "73")
        .unwrap()
        .value
        .unwrap();
    assert_eq!(journal.phase, PlaylistPhase::Sending);
    assert_eq!(fixture.api.current(&session), before);
    assert!(matches!(
        client
            .reorder_playlist_mixed(
                "1",
                "73",
                &before.revision,
                vec![fixture.local(1), fixture.local(0)],
            )
            .await,
        Err(Error::MutationBusy)
    ));
    assert_eq!(
        *fixture.api.writes.lock().unwrap(),
        [json!({"pid":"73","op":"update","trackIds":"[]"})]
    );
    fixture.api.release.notify_one();
    let receipt = pending.await.unwrap().unwrap().value;
    assert_eq!(receipt.operation_id, journal.operation_id);
    assert_eq!(receipt.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        receipt.evidence,
        Some(PlaylistMixedReorderEvidence::Acknowledged)
    );
    assert_eq!(receipt.service_code, Some(200));
    let after = fixture.api.current(&session);
    assert_eq!(
        after.rows,
        [
            PlaylistLocalRow::Local(fixture.rows[1].clone()),
            PlaylistLocalRow::Local(fixture.rows[0].clone())
        ]
    );
    assert_ne!(after.revision, before.revision);
}

#[tokio::test]
async fn mixed_reorder_can_recover_only_changed_exact_online_order_after_reopen() {
    let fixture = Fixture::new(&[10, 20], Reply::Lost).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    let receipt = client
        .reorder_playlist_mixed(
            "1",
            "73",
            &before.revision,
            vec![online(20), fixture.local(1), online(10), fixture.local(0)],
        )
        .await
        .unwrap()
        .value;
    assert_eq!(receipt.phase, PlaylistPhase::Uncertain);
    assert_eq!(receipt.evidence, None);
    let remote_changed = fixture.api.current(&session);
    assert_eq!(remote_changed.generation(), before.generation());
    assert_eq!(remote_changed.rows, before.rows);
    assert!(matches!(
        client
            .mutate_playlist_local("1", "73", &remote_changed.revision, remove(&fixture.rows))
            .await,
        Err(Error::MutationUncertain)
    ));
    drop(client);
    drop(session);

    let (client, session) = fixture.open();
    assert_eq!(
        client
            .playlist_mixed_reorder_receipt("1", "73")
            .unwrap()
            .value,
        Some(receipt.clone())
    );
    let (other, _, _) = open(&fixture.profile, fixture.api.clone(), Some(10));
    assert!(
        other
            .playlist_mixed_reorder_receipt("1", "73")
            .unwrap()
            .value
            .is_none()
    );
    assert!(matches!(
        other
            .reconcile_playlist_mixed_reorder("1", "73", &receipt.operation_id)
            .await,
        Err(Error::StaleResponse)
    ));
    let observed_target = std::mem::replace(
        &mut fixture.api.detail.lock().unwrap()["trackIds"],
        json!([{"id":10,"v":0},{"id":20,"v":0}]),
    );
    let not_yet = client
        .reconcile_playlist_mixed_reorder("1", "73", &receipt.operation_id)
        .await
        .unwrap()
        .value;
    assert_eq!(not_yet, receipt);
    assert_eq!(fixture.api.current(&session), before);
    fixture.api.detail.lock().unwrap()["trackIds"] = observed_target;
    let recovered = client
        .reconcile_playlist_mixed_reorder("1", "73", &receipt.operation_id)
        .await
        .unwrap()
        .value;
    assert_eq!(recovered.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        recovered.evidence,
        Some(PlaylistMixedReorderEvidence::ChangedReadback)
    );
    assert_eq!(
        fixture.api.current(&session).rows,
        [
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Local(fixture.rows[1].clone()),
            PlaylistLocalRow::Remote { index: 1 },
            PlaylistLocalRow::Local(fixture.rows[0].clone()),
        ]
    );
    assert_eq!(
        *fixture.api.writes.lock().unwrap(),
        [json!({"pid":"73","op":"update","trackIds":"[\"20\",\"10\"]"})]
    );
}

#[tokio::test]
async fn mixed_reorder_preserves_the_old_order_on_equal_unknown_or_known_rejection() {
    for (reply, phase, evidence) in [
        (Reply::Lost, PlaylistPhase::Uncertain, None),
        (Reply::MissingCode, PlaylistPhase::Uncertain, None),
        (
            Reply::Reject,
            PlaylistPhase::Rejected,
            Some(PlaylistMixedReorderEvidence::ExplicitRejection),
        ),
        (
            Reply::NotSent,
            PlaylistPhase::Rejected,
            Some(PlaylistMixedReorderEvidence::NotSent),
        ),
    ] {
        let fixture = Fixture::new(&[10, 20], reply).await;
        let (client, session) = fixture.open();
        let before = fixture.api.current(&session);
        let desired = || vec![online(10), fixture.local(1), online(20), fixture.local(0)];
        let receipt = client
            .reorder_playlist_mixed("1", "73", &before.revision, desired())
            .await
            .unwrap()
            .value;
        assert_eq!(receipt.phase, phase);
        assert_eq!(receipt.evidence, evidence);
        let reconciled = client
            .reconcile_playlist_mixed_reorder("1", "73", &receipt.operation_id)
            .await
            .unwrap()
            .value;
        assert_eq!(reconciled, receipt);
        assert_eq!(fixture.api.current(&session), before);
        if phase == PlaylistPhase::Uncertain {
            assert!(matches!(
                client
                    .reorder_playlist_mixed("1", "73", &before.revision, desired())
                    .await,
                Err(Error::MutationUncertain)
            ));
            let editor = client.playlist_editor("1", "73").await.unwrap().value;
            assert!(matches!(
                client
                    .mutate_playlist(
                        "1",
                        PlaylistCommand::Delete {
                            id: "73".into(),
                            revision: editor.revision
                        }
                    )
                    .await,
                Err(Error::MutationUncertain)
            ));
        }
        assert_eq!(
            *fixture.api.writes.lock().unwrap(),
            [json!({"pid":"73","op":"update","trackIds":"[\"10\",\"20\"]"})]
        );
    }
}

#[tokio::test]
async fn mixed_reorder_does_not_send_unbound_rows_or_a_non_permutation() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, session) = fixture.open();
    let before = fixture.api.current(&session);
    assert!(
        serde_json::from_value::<PlaylistMixedOrderRow>(json!({
            "kind":"local", "id":fixture.rows[0].id,
            "revision":fixture.rows[0].revision, "path":"untrusted.wav"
        }))
        .is_err()
    );
    assert!(matches!(
        client
            .reorder_playlist_mixed(
                "1",
                "73",
                &before.revision,
                vec![online(10), fixture.local(1), online(99), fixture.local(0)]
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        client
            .reorder_playlist_mixed(
                "1",
                "73",
                &before.revision,
                vec![online(10), fixture.local(1), online(20)]
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    fixture.api.detail.lock().unwrap()["trackIds"][0]["v"] = json!(1);
    assert!(matches!(
        client
            .reorder_playlist_mixed(
                "1",
                "73",
                &before.revision,
                vec![online(10), fixture.local(1), online(20), fixture.local(0)]
            )
            .await,
        Err(Error::QueueChanged)
    ));
    fixture.api.detail.lock().unwrap()["trackIds"][0]["v"] = json!(-1);
    let before = fixture.api.current(&session);
    assert!(matches!(
        client
            .reorder_playlist_mixed(
                "1",
                "73",
                &before.revision,
                vec![online(10), fixture.local(1), online(20), fixture.local(0)]
            )
            .await,
        Err(Error::State(
            "cannot reorder unbound server local playlist rows"
        ))
    ));
    assert!(fixture.api.writes.lock().unwrap().is_empty());
}
