use super::*;
use cumulus_core::{client::ClientError, playback::controller::PlaybackStatus};

struct Batch {
    epoch: String,
    receipt: String,
    revision: String,
}

impl Batch {
    async fn open(fixture: &Fixture, loaded: usize) -> Self {
        let (epoch, receipt) = fixture.source().await;
        let revision = fixture
            .client
            .vertical_zone_playlist_detail(&epoch, &receipt, "204", page(0, loaded, None))
            .await
            .unwrap()
            .value
            .revision
            .unwrap();
        fixture.wire.take_calls();
        Self {
            epoch,
            receipt,
            revision,
        }
    }

    fn selection(&self, ids: &[&str]) -> PlaySelection {
        PlaySelection::VerticalZonePlaylistBatch {
            id: "204".into(),
            receipt: self.receipt.clone(),
            ids: ids.iter().map(|id| id.to_string()).collect(),
            revision: self.revision.clone(),
            quality: Quality::Standard,
        }
    }

    fn append(&self, ids: &[&str], queue_revision: &str) -> PlayerAction {
        PlayerAction::QueueVerticalZonePlaylistBatch {
            id: "204".into(),
            receipt: self.receipt.clone(),
            ids: ids.iter().map(|id| id.to_string()).collect(),
            revision: self.revision.clone(),
            quality: Quality::Standard,
            expected_queue_revision: queue_revision.into(),
        }
    }

    fn page(&self) -> PageRequest {
        page(0, 3, Some(self.revision.clone()))
    }
}

async fn ordinary(f: &Fixture, ids: &[&str]) -> PlayerView {
    let value = f
        .client
        .play_selection(
            &f.epoch(),
            PlaySelection::Songs {
                ids: ids.iter().map(|id| id.to_string()).collect(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let calls = await_url(&f.wire, -1).await;
    assert!(
        calls
            .iter()
            .any(|(path, args)| path == SONG && args.get("trialMode").is_none())
    );
    value
}

fn order(queue: &QueueView) -> Vec<&str> {
    queue
        .tracks
        .iter()
        .map(|entry| entry.track.id.as_str())
        .collect()
}

fn assert_player_url(calls: &[(String, Value)], id: &str, mode: i32) {
    assert!(calls.iter().any(|(path, args)| {
        path == URL
            && args["trialMode"] == mode
            && serde_json::from_str::<Vec<Value>>(args["ids"].as_str().unwrap())
                .unwrap()
                .first()
                .is_some_and(|value| value == id)
    }));
}

#[tokio::test]
async fn batch_hydrates_unfilled_rows_with_default_trial_and_never_rereads_details() {
    let f = Fixture::new();
    f.wire.omit_121.store(102, Ordering::Release);
    let batch = Batch::open(&f, 2).await;
    for revision in [None, Some("not-the-revision".into())] {
        assert!(
            f.client
                .vertical_zone_playlist_batch_page(
                    &batch.epoch,
                    &batch.receipt,
                    "204",
                    page(0, 3, revision),
                )
                .await
                .is_err()
        );
    }
    assert!(f.wire.take_calls().is_empty());
    let rows = f
        .client
        .vertical_zone_playlist_batch_page(&batch.epoch, &batch.receipt, "204", batch.page())
        .await
        .unwrap()
        .value;
    assert_eq!(rows.revision.as_deref(), Some(batch.revision.as_str()));
    assert_eq!(
        rows.tracks
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["101", "102", "103"]
    );
    assert!(!rows.tracks[0].missing && !rows.tracks[1].missing);
    assert!(rows.tracks[2].local && rows.tracks[2].missing);
    assert_eq!(f.wire.take_calls(), vec![song_trial(&["102"], Some(-1))]);
}

#[tokio::test]
async fn batch_play_keeps_other_queue_rows_and_uses_authoritative_order_and_trial() {
    let f = Fixture::new();
    let batch = Batch::open(&f, 2).await;
    ordinary(&f, &["900", "901"]).await;
    let played = f
        .client
        .play_selection(&batch.epoch, batch.selection(&["102", "101"]))
        .await
        .unwrap();
    assert_eq!(played.track.unwrap().id, "101");
    assert_eq!(
        order(&f.client.player_queue(&batch.epoch).await.unwrap()),
        ["900", "101", "102", "901"]
    );
    let calls = await_url(&f.wire, 121).await;
    assert!(calls.contains(&song_trial(&["101", "102"], Some(121))));
    assert_player_url(&calls, "101", 121);
    assert!(
        !calls
            .iter()
            .any(|(path, _)| matches!(path.as_str(), V4 | V6 | DYNAMIC))
    );
}

#[tokio::test]
async fn own_liked_append_keeps_current_paused_and_rebinds_a_deduplicated_row_to_43() {
    let f = Fixture::new();
    f.wire.membership.store(2, Ordering::Release);
    f.wire.own.store(true, Ordering::Release);
    f.client.playlists().await.unwrap();
    let batch = Batch::open(&f, 2).await;
    ordinary(&f, &["900", "102", "901"]).await;
    let paused = f
        .client
        .player_action(&batch.epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before = f.client.player_queue(&batch.epoch).await.unwrap();
    let moved_id = before.tracks[1].entry_id.clone();
    f.wire.take_calls();
    assert!(matches!(
        f.client
            .player_action(&batch.epoch, batch.append(&["102", "101"], "0"))
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(f.wire.take_calls().is_empty());
    let added = f
        .client
        .player_action(
            &batch.epoch,
            batch.append(&["102", "101"], &paused.queue_revision),
        )
        .await
        .unwrap();
    assert_eq!(
        (added.state, added.play_id, added.position),
        (paused.state, paused.play_id, paused.position)
    );
    let after = f.client.player_queue(&batch.epoch).await.unwrap();
    assert_eq!(order(&after), ["900", "101", "102", "901"]);
    assert_eq!(after.tracks[2].entry_id, moved_id);
    let added_wire = f.wire.take_calls();
    assert!(added_wire.contains(&song_trial(&["101", "102"], Some(43))));
    assert!(
        !added_wire
            .iter()
            .any(|(path, _)| matches!(path.as_str(), V4 | V6 | DYNAMIC))
    );
    let selected = f
        .client
        .player_action(
            &batch.epoch,
            PlayerAction::QueueFor {
                expected_queue_revision: after.revision,
                expected_room_id: None,
                edit: QueueEdit::Select { entry_id: moved_id },
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.track.unwrap().id, "102");
    let calls = await_url(&f.wire, 43).await;
    assert_player_url(&calls, "102", 43);
}

#[tokio::test]
async fn forged_local_or_missing_membership_never_commits_a_partial_selection() {
    let f = Fixture::new();
    let batch = Batch::open(&f, 1).await;
    let started = ordinary(&f, &["900"]).await;
    f.wire.take_calls();
    for ids in [&["101", "103"][..], &["101", "999"][..]] {
        assert!(matches!(
            f.client
                .play_selection(&batch.epoch, batch.selection(ids))
                .await,
            Err(Error::QueueChanged)
        ));
        assert!(f.wire.take_calls().is_empty());
    }
    f.wire.omit_121.store(102, Ordering::Release);
    assert!(matches!(
        f.client
            .play_selection(&batch.epoch, batch.selection(&["101", "102"]))
            .await,
        Err(Error::State("selected zone playlist songs are unavailable"))
    ));
    assert_eq!(
        f.wire.take_calls(),
        vec![song_trial(&["101", "102"], Some(121))]
    );
    assert_eq!(
        order(&f.client.player_queue(&batch.epoch).await.unwrap()),
        ["900"]
    );
    assert_eq!(
        f.client
            .player_snapshot(&batch.epoch)
            .await
            .unwrap()
            .play_id,
        started.play_id
    );
}

#[tokio::test]
async fn native_url_denial_never_loads_the_zone_song_even_when_metadata_exists() {
    let f = Fixture::new();
    let batch = Batch::open(&f, 1).await;
    f.wire.deny_121.store(true, Ordering::Release);
    let failure = f
        .client
        .play_selection(&batch.epoch, batch.selection(&["101"]))
        .await
        .err()
        .unwrap();
    assert_eq!(
        ClientError::from(failure).code,
        "playback_source_unavailable"
    );
    let current = f.client.player_snapshot(&batch.epoch).await.unwrap();
    assert_eq!(current.state, PlaybackStatus::Error);
    assert_eq!(f.device.state.lock().unwrap().loads, 0);
    assert_player_url(&f.wire.take_calls(), "101", 121);
}

#[tokio::test]
async fn account_change_cancels_hydration_and_detail_refresh_discards_same_digest_pending_action() {
    let f = Fixture::new();
    let batch = Batch::open(&f, 1).await;
    f.wire.hold_trial.store(-1, Ordering::Release);
    let pending = f.client.vertical_zone_playlist_batch_page(
        &batch.epoch,
        &batch.receipt,
        "204",
        batch.page(),
    );
    let switch = async {
        f.wire.held.notified().await;
        f.adopt(8);
        f.wire.release.notify_one();
    };
    let (page, _) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(pending, switch)
    })
    .await
    .unwrap();
    assert!(matches!(page, Err(Error::StaleResponse)));
    assert!(matches!(
        f.client
            .play_selection(&batch.epoch, batch.selection(&["101"]))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .player_queue(&f.epoch())
            .await
            .unwrap()
            .tracks
            .is_empty()
    );

    let f = Fixture::new();
    let batch = Batch::open(&f, 1).await;
    let current = ordinary(&f, &["900"]).await;
    f.wire.hold_trial.store(121, Ordering::Release);
    let pending = f.client.player_action(
        &batch.epoch,
        batch.append(&["101"], &current.queue_revision),
    );
    let refresh = async {
        f.wire.held.notified().await;
        let refreshed = f
            .client
            .vertical_zone_playlist_detail(
                &batch.epoch,
                &batch.receipt,
                "204",
                super::page(0, 1, None),
            )
            .await
            .unwrap()
            .value;
        assert_eq!(refreshed.revision.as_deref(), Some(batch.revision.as_str()));
        f.wire.release.notify_one();
    };
    let (appended, _) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(pending, refresh)
    })
    .await
    .unwrap();
    assert!(matches!(appended, Err(Error::StaleResponse)));
    assert_eq!(
        order(&f.client.player_queue(&batch.epoch).await.unwrap()),
        ["900"]
    );
    assert!(
        !f.wire
            .take_calls()
            .iter()
            .any(|(path, args)| path == URL && args["trialMode"] == 121)
    );
}

#[tokio::test]
async fn an_in_flight_menu_read_does_not_revoke_committed_batch_authority() {
    let f = Fixture::new();
    let batch = Batch::open(&f, 1).await;
    f.wire.hold_trial.store(121, Ordering::Release);
    let (completed, wait_completed) = tokio::sync::oneshot::channel();
    let start = async {
        let result = f
            .client
            .play_selection(&batch.epoch, batch.selection(&["101"]))
            .await;
        completed.send(()).unwrap();
        result
    };
    let refresh = async {
        f.wire.held.notified().await;
        f.wire.hold_menu.store(true, Ordering::Release);
        let menu = f.client.vertical_zone_menu(&batch.epoch);
        let during_refresh = async {
            f.wire.menu_held.notified().await;
            f.wire.release.notify_one();
            wait_completed.await.unwrap();
            f.wire.menu_release.notify_one();
        };
        let (menu, ()) = tokio::join!(menu, during_refresh);
        assert!(menu.unwrap().value.available);
    };
    let (played, ()) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(start, refresh)
    })
    .await
    .unwrap();
    assert_eq!(played.unwrap().track.unwrap().id, "101");

    f.wire.removed.store(true, Ordering::Release);
    f.client.vertical_zone_menu(&batch.epoch).await.unwrap();
    f.wire.take_calls();
    assert!(matches!(
        f.client
            .play_selection(&batch.epoch, batch.selection(&["101"]))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.wire.take_calls().is_empty());
}
