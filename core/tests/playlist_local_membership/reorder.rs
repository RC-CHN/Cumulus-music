use super::*;
use cumulus_core::playlist::{PlaylistMixedOrderRow, PlaylistMixedReorderEvidence, PlaylistPhase};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::Notify;

const UPDATE: &str = "/api/v1/playlist/manipulate/tracks";

#[derive(Clone, Copy)]
enum Reply {
    Ack,
    Lost,
    Reject,
    NotSent,
    MissingCode,
}

struct ReorderServer {
    detail: Mutex<Value>,
    writes: Mutex<Vec<Value>>,
    reply: Reply,
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
            reply,
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
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            match path {
                "/api/v6/playlist/detail" => {
                    assert_eq!(payload, json!({"id":"73","n":100000,"s":0}));
                    Ok(json!({"code":200,"playlist":self.detail.lock().unwrap().clone()}))
                }
                UPDATE => {
                    assert_eq!(payload["pid"], "73");
                    assert_eq!(payload["op"], "update");
                    assert!(payload.get("imme").is_none());
                    let ids: Vec<String> =
                        serde_json::from_str(payload["trackIds"].as_str().unwrap()).unwrap();
                    self.writes.lock().unwrap().push(payload);
                    if self.hold.swap(false, Ordering::SeqCst) {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    if matches!(self.reply, Reply::Ack | Reply::Lost | Reply::MissingCode) {
                        let mut detail = self.detail.lock().unwrap();
                        let original = detail["trackIds"].as_array().unwrap();
                        detail["trackIds"] = json!(
                            ids.iter()
                                .map(|id| {
                                    original
                                        .iter()
                                        .find(|row| row["id"].to_string().trim_matches('"') == id)
                                        .expect("update leaked local ID or changed membership")
                                        .clone()
                                })
                                .collect::<Vec<_>>()
                        );
                    }
                    match self.reply {
                        Reply::Ack => Ok(json!({"code":"200"})),
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
