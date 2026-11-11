//! The actual ordinary playback selection/authority with a synthetic transport.
use super::*;
use cumulus_core::playback::controller::PlaybackStatus;
use std::{sync::atomic::AtomicI64, time::Duration};

#[derive(Default)]
pub(super) struct Server {
    revision: AtomicU64,
    share: AtomicI64,
    replacement_id: AtomicU64,
    private_reply: AtomicU64,
    hold: AtomicU64,
    started: Notify,
    release: Notify,
    calls: Mutex<Vec<(String, String)>>,
}

impl Server {
    fn start(&self) {
        self.revision.store(1, Ordering::SeqCst);
        self.share.store(1, Ordering::SeqCst);
    }
    pub(super) fn revision(&self) -> Option<String> {
        let revision = self.revision.load(Ordering::SeqCst);
        (revision > 0).then(|| format!("synthetic-player-share-{revision}"))
    }
    pub(super) fn annotate_details(&self, response: &mut Value) {
        if self.revision().is_none() {
            return;
        }
        let privileges: Vec<_> = response["songs"].as_array_mut().unwrap().iter_mut().map(|song| {
            let id = song["id"].as_str().unwrap().parse::<u64>().unwrap();
            if id == 4 {
                song["onlineTrackId"] = json!(7);
            }
            json!({"id":id,"st":if id==3 {-200} else {0},"fee":if id==2 {4} else {0},"pl":320000})
        }).collect();
        response["privileges"] = json!(privileges);
    }
    pub(super) async fn access(
        &self,
        id: &str,
        revision: &str,
        permission: &(dyn Fn() -> cumulus_core::Result<()> + Send + Sync),
    ) -> cumulus_core::Result<Value> {
        if self.hold.load(Ordering::SeqCst) == 1 {
            self.started.notify_one();
            self.release.notified().await;
        }
        permission()?;
        if self.revision().as_deref() != Some(revision) {
            return Err(Error::CredentialChanged);
        }
        self.calls
            .lock()
            .unwrap()
            .push((id.to_owned(), revision.to_owned()));
        if self.hold.load(Ordering::SeqCst) == 2 {
            self.started.notify_one();
            self.release.notified().await;
        }
        let replacement = self.replacement_id.load(Ordering::SeqCst);
        let result_id = if replacement == 0 {
            id.to_owned()
        } else {
            replacement.to_string()
        };
        Ok(json!({"code":200,"data":[{
            "id":result_id,"sp":self.share.load(Ordering::SeqCst),"cs":self.private_reply.load(Ordering::SeqCst)
        }]}))
    }
}

fn target(play: &PlayerView) -> PlayerSongShareTarget {
    serde_json::from_value(json!({"playId":play.play_id,"entryId":play.entry_id})).unwrap()
}
fn receipt(value: HistoryRowSharePreparation) -> String {
    match value {
        HistoryRowSharePreparation::Ready { receipt } => receipt,
        other => panic!("expected a share offer, got {other:?}"),
    }
}
async fn paused(client: &MusicClient, id: &str) -> PlayerView {
    let view = client.play_selection("1", songs(id)).await.unwrap();
    client
        .player_action(
            "1",
            PlayerAction::PauseFor {
                play_id: view.play_id.unwrap(),
                expected_queue_revision: view.queue_revision,
                paused: true,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn current_song_share_has_two_reads_per_channel_and_preserves_paused_playback() {
    let (_root, api, _, client, _) = fixture();
    let wire = &api.player_song_share;
    wire.start();
    let before = paused(&client, "1").await;
    assert!(before.current_song_share_available);
    let selected = target(&before);
    assert!(
        wire.calls.lock().unwrap().is_empty(),
        "ordinary loads do not read share privileges"
    );
    let forged =
        serde_json::from_value(json!({"playId":before.play_id,"entryId":"incorrect"})).unwrap();
    assert!(matches!(
        client.player_song_share_prepare("1", forged).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .player_song_share_prepare("0", selected.clone())
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(wire.calls.lock().unwrap().is_empty());

    let cancelled = receipt(
        client
            .player_song_share_prepare("1", selected.clone())
            .await
            .unwrap()
            .value,
    );
    client
        .player_song_share_cancel("1", selected.clone(), &cancelled)
        .unwrap();
    assert!(matches!(
        client
            .player_song_share_choose(
                "1",
                selected.clone(),
                &cancelled,
                HistoryRowShareChoice::Copy
            )
            .await,
        Err(Error::StaleResponse)
    ));

    let denied = receipt(
        client
            .player_song_share_prepare("1", selected.clone())
            .await
            .unwrap()
            .value,
    );
    wire.share.store(0, Ordering::SeqCst);
    let result = client
        .player_song_share_choose("1", selected.clone(), &denied, HistoryRowShareChoice::Copy)
        .await
        .unwrap();
    assert_eq!(result.value, HistoryRowShareOutcome::Unavailable);
    assert!(matches!(
        client
            .player_song_share_choose("1", selected.clone(), &denied, HistoryRowShareChoice::Copy)
            .await,
        Err(Error::StaleResponse)
    ));

    wire.share.store(1, Ordering::SeqCst);
    for choice in [HistoryRowShareChoice::Copy, HistoryRowShareChoice::WechatQr] {
        let ready = receipt(
            client
                .player_song_share_prepare("1", selected.clone())
                .await
                .unwrap()
                .value,
        );
        let result = client
            .player_song_share_choose("1", selected.clone(), &ready, choice)
            .await
            .unwrap();
        match (choice, result.value) {
            (HistoryRowShareChoice::Copy, HistoryRowShareOutcome::Copy { url }) => {
                assert_eq!(url, "https://music.163.com/song?id=1")
            }
            (HistoryRowShareChoice::WechatQr, HistoryRowShareOutcome::WechatQr { modules }) => {
                assert!(modules.len() == 25 && modules.iter().all(|row| row.len() == 25))
            }
            (_, unexpected) => panic!("unexpected channel result: {unexpected:?}"),
        }
    }
    assert_eq!(
        *wire.calls.lock().unwrap(),
        vec![("1".to_owned(), "synthetic-player-share-1".to_owned()); 7]
    );
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.entry_id, before.entry_id);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(after.position, before.position);
}

#[tokio::test]
async fn current_song_share_has_no_private_or_unverified_fallback() {
    let (_root, api, _, client, _) = fixture();
    let wire = &api.player_song_share;
    let no_evidence = paused(&client, "1").await;
    assert!(!no_evidence.current_song_share_available);
    assert_eq!(
        client
            .player_song_share_prepare("1", target(&no_evidence))
            .await
            .unwrap()
            .value,
        HistoryRowSharePreparation::Unavailable
    );

    wire.start();
    for id in ["3", "4"] {
        let view = paused(&client, id).await;
        assert!(!view.current_song_share_available);
        assert_eq!(
            client
                .player_song_share_prepare("1", target(&view))
                .await
                .unwrap()
                .value,
            HistoryRowSharePreparation::Unavailable
        );
    }
    assert!(wire.calls.lock().unwrap().is_empty());

    let album = paused(&client, "2").await;
    assert!(album.current_song_share_available);
    wire.share.store(0, Ordering::SeqCst);
    assert_eq!(
        client
            .player_song_share_prepare("1", target(&album))
            .await
            .unwrap()
            .value,
        HistoryRowSharePreparation::AlbumRequired
    );
    wire.share.store(1, Ordering::SeqCst);
    wire.replacement_id.store(7, Ordering::SeqCst);
    assert!(matches!(
        client.player_song_share_prepare("1", target(&album)).await,
        Err(Error::Response(_))
    ));
    wire.replacement_id.store(0, Ordering::SeqCst);
    wire.private_reply.store(1, Ordering::SeqCst);
    assert_eq!(
        client
            .player_song_share_prepare("1", target(&album))
            .await
            .unwrap()
            .value,
        HistoryRowSharePreparation::Unavailable
    );
    assert_eq!(wire.calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn queued_or_late_share_is_fenced_by_cancel_play_and_credential_changes() {
    let (_root, api, session, client, _) = fixture();
    let wire = &api.player_song_share;
    wire.start();
    let mut selected = target(&paused(&client, "1").await);
    for (hold, invalidate, reads) in [
        (1, "cancel", 1),
        (2, "play", 3),
        (2, "credential", 5),
        (2, "epoch", 7),
    ] {
        let ready = receipt(
            client
                .player_song_share_prepare("1", selected.clone())
                .await
                .unwrap()
                .value,
        );
        wire.hold.store(hold, Ordering::SeqCst);
        let host_actions = Arc::new(AtomicU64::new(0));
        let task = tokio::spawn({
            let client = client.clone();
            let selected = selected.clone();
            let ready = ready.clone();
            let host_actions = host_actions.clone();
            async move {
                client
                    .player_song_share_choose_with(
                        "1",
                        selected,
                        &ready,
                        HistoryRowShareChoice::Copy,
                        move |_| {
                            host_actions.fetch_add(1, Ordering::SeqCst);
                        },
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), wire.started.notified())
            .await
            .unwrap();
        match invalidate {
            "cancel" => client
                .player_song_share_cancel("1", selected.clone(), &ready)
                .unwrap(),
            "play" => selected = target(&paused(&client, "2").await),
            "credential" => wire.revision.store(2, Ordering::SeqCst),
            "epoch" => {
                session
                    .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
                    .unwrap();
            }
            _ => unreachable!(),
        }
        wire.release.notify_one();
        assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
        assert_eq!(host_actions.load(Ordering::SeqCst), 0);
        assert_eq!(wire.calls.lock().unwrap().len(), reads);
        wire.hold.store(0, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn a_new_host_subscription_cannot_consume_the_old_ready_offer() {
    use cumulus_core::playback::engine::Cancellation;
    let (_root, api, _, client, _) = fixture();
    let wire = &api.player_song_share;
    wire.start();
    let selected = target(&paused(&client, "1").await);
    let original = Cancellation::default();
    let old = receipt(
        client
            .player_song_share_prepare_with_guard("1", selected.clone(), original.clone())
            .await
            .unwrap()
            .value,
    );
    original.cancel();
    let mut applied = false;
    assert!(matches!(
        client
            .player_song_share_choose_with(
                "1",
                selected.clone(),
                &old,
                HistoryRowShareChoice::Copy,
                |_| applied = true
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(!applied);
    assert_eq!(wire.calls.lock().unwrap().len(), 1);

    let replacement = Cancellation::default();
    let new = receipt(
        client
            .player_song_share_prepare_with_guard("1", selected.clone(), replacement)
            .await
            .unwrap()
            .value,
    );
    client
        .player_song_share_cancel("1", selected.clone(), &old)
        .unwrap();
    assert_ne!(old, new);
    let result = client
        .player_song_share_choose_with(
            "1",
            selected,
            &new,
            HistoryRowShareChoice::Copy,
            |outcome| {
                applied = true;
                outcome
            },
        )
        .await
        .unwrap();
    assert!(applied);
    assert_eq!(
        result.value,
        HistoryRowShareOutcome::Copy {
            url: "https://music.163.com/song?id=1".into()
        }
    );
    assert_eq!(wire.calls.lock().unwrap().len(), 3);
}
