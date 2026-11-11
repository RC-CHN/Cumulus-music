use super::{row_next::commit, *};
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};

async fn row_play(f: &Fixture, kind: HistoryKind, clicked: &str, ids: &[&str]) -> PlaySelection {
    let page = f
        .client
        .history_page(&f.epoch(), kind, false)
        .await
        .unwrap();
    PlaySelection::HistoryRowPlay {
        history_kind: kind,
        revision: page.value.revision,
        clicked_id: clicked.into(),
        selected_ids: ids.iter().map(|id| (*id).into()).collect(),
        quality: Quality::Standard,
    }
}

async fn start(f: &Fixture, kind: HistoryKind, ids: &[&str], index: usize) -> PlayerView {
    let mut selection = f
        .selection(kind, ids.iter().map(|id| (*id).into()).collect())
        .await;
    if let PlaySelection::History { index: anchor, .. } = &mut selection {
        *anchor = index;
    }
    f.client
        .play_selection(&f.epoch(), selection)
        .await
        .unwrap()
}

#[tokio::test]
async fn row_play_inserts_a_song_group_and_starts_the_clicked_existing_entry() {
    let f = fixture(true);
    let epoch = f.epoch();
    let kind = HistoryKind::Song;
    seed_history_batch(
        &f,
        kind,
        &[(1, 500), (2, 400), (3, 300), (4, 200), (5, 100)],
    );
    let incumbent = start(&f, kind, &["1", "4", "5"], 1).await;
    let queue = f.client.player_queue(&epoch).await.unwrap();
    let old_clicked_entry = queue.tracks[0].entry_id.clone();
    f.client
        .player_action(
            &epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    f.client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();

    let playing = commit(&f, row_play(&f, kind, "1", &["3", "1"]).await)
        .await
        .unwrap();
    assert_eq!(history_batch_ids(&f).await, ["4", "3", "1", "5"]);
    assert_eq!(playing.track.unwrap().id, "1");
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert_eq!(playing.mode, PlaybackMode::Random);
    assert_eq!(
        playing.entry_id.as_deref(),
        Some(old_clicked_entry.as_str())
    );
    assert_ne!(playing.play_id, incumbent.play_id);
}

#[tokio::test]
async fn row_play_resumes_the_same_loaded_episode_without_replacing_the_decoder() {
    let f = fixture(true);
    let epoch = f.epoch();
    let kind = HistoryKind::Voice;
    seed_history_batch(&f, kind, &[(5, 500), (6, 400), (7, 300), (8, 200)]);
    start(&f, kind, &["5", "6", "7"], 1).await;
    f.client
        .player_action(&epoch, PlayerAction::Seek { seconds: 16.0 })
        .await
        .unwrap();
    let paused = f
        .client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let loaded = f.device.state.lock().unwrap().loads;

    let resumed = commit(&f, row_play(&f, kind, "6", &["8", "6"]).await)
        .await
        .unwrap();
    assert_eq!(history_batch_ids(&f).await, ["5", "8", "6", "7"]);
    assert_eq!(resumed.track.unwrap().voice_id.as_deref(), Some("6"));
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.entry_id, paused.entry_id);
    assert_eq!(resumed.play_id, paused.play_id);
    assert_eq!(resumed.position, paused.position);
    assert_eq!(f.device.state.lock().unwrap().loads, loaded);

    let already_playing = commit(&f, row_play(&f, kind, "6", &[]).await)
        .await
        .unwrap();
    assert_eq!(already_playing.state, PlaybackStatus::Playing);
    assert_eq!(already_playing.play_id, resumed.play_id);
    assert_eq!(already_playing.entry_id, resumed.entry_id);
    assert_eq!(f.device.state.lock().unwrap().loads, loaded);
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/v6/dj/program/byradio")
    );
}

#[tokio::test]
async fn row_play_projects_copyright_separately_from_membership_and_rejects_the_denied_anchor() {
    let f = fixture(true);
    let epoch = f.epoch();
    let kind = HistoryKind::Song;
    for (id, at, status, fee) in [(1, 600, 0, 0), (2, 500, -100, 0), (3, 400, 0, 1)] {
        let mut raw = song(&json!(id));
        raw["privilege"] = json!({"status":status,"fee":fee});
        f.history
            .record_for(
                &f.session.token().unwrap(),
                kind,
                HistoryRecord::local(kind, raw, at).unwrap(),
            )
            .unwrap();
    }
    let page = f.client.history_page(&epoch, kind, false).await.unwrap();
    let view = serde_json::to_value(page.value).unwrap();
    let entries = view["entries"].as_array().unwrap();
    assert!(
        entries.iter().find(|row| row["id"] == "2").unwrap()["songMenuCopyrightBlocked"] == true
    );
    assert!(
        entries.iter().find(|row| row["id"] == "3").unwrap()["songMenuCopyrightBlocked"] != true
    );
    start(&f, kind, &["1"], 0).await;
    let paused = f
        .client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    *f.api.blocked_songs.lock().unwrap() = vec!["2".into()];
    assert!(
        commit(&f, row_play(&f, kind, "2", &["3", "2"]).await)
            .await
            .is_err()
    );
    let now = f.client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(now.queue_revision, paused.queue_revision);
    assert_eq!(now.play_id, paused.play_id);
    assert_eq!(now.state, PlaybackStatus::Paused);
    assert_eq!(history_batch_ids(&f).await, ["1"]);
}
