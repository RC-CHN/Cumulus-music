use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

fn row(id: &str) -> PlaySelection {
    PlaySelection::SongRowDoubleClick {
        selection: SongRowSelection::Playlist {
            id: "7".into(),
            start_id: id.into(),
            revision: None,
            quality: Quality::Lossless,
        },
    }
}

fn setting(revision: &str, replace_queue: bool) -> QueueClickPreferencesCommand {
    QueueClickPreferencesCommand::Change {
        expected_revision: revision.into(),
        replace_queue,
    }
}

async fn queue(client: &MusicClient) -> Vec<(String, String)> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|row| (row.track.id, row.entry_id))
        .collect()
}

#[tokio::test]
async fn default_replace_keep_only_the_selected_song_reuse_and_explicit_play() {
    let (_dir, api, _, client, _) = fixture();
    *api.playlist_tracks.lock().unwrap() = Some((1..=3).map(|id| json!({"id":id})).collect());
    client.play_selection("1", songs("99")).await.unwrap();
    let view = client.play_selection("1", row("2")).await.unwrap();
    assert_eq!(view.track.unwrap().id, "2");
    let original = queue(&client).await;
    assert_eq!(
        original
            .iter()
            .map(|row| row.0.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    client
        .queue_click_preferences_command("1", setting("0", false))
        .unwrap();

    *api.playlist_tracks.lock().unwrap() =
        Some([4, 2, 5].into_iter().map(|id| json!({"id":id})).collect());
    let added = client.play_selection("1", row("4")).await.unwrap();
    assert_eq!(added.track.unwrap().id, "4");
    assert_eq!(added.state, PlaybackStatus::Playing);
    let retained = queue(&client).await;
    assert_eq!(&retained[..3], &original);
    assert_eq!(retained.len(), 4);
    assert_eq!(retained[3].0, "4");
    let reused = client.play_selection("1", row("2")).await.unwrap();
    assert_eq!(reused.entry_id.as_deref(), Some(original[1].1.as_str()));
    assert_eq!(queue(&client).await, retained);

    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let resumed = client.play_selection("1", row("2")).await.unwrap();
    assert_eq!(resumed.play_id, paused.play_id);
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(queue(&client).await, retained);

    client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "7".into(),
                start_id: None,
                revision: None,
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        queue(&client)
            .await
            .iter()
            .map(|row| row.0.as_str())
            .collect::<Vec<_>>(),
        ["4", "2", "5"]
    );
}

#[tokio::test]
async fn late_selected_song_cannot_insert_into_a_replacement_queue() {
    let (_dir, api, _, client, _) = fixture();
    client.play_selection("1", songs("99")).await.unwrap();
    client
        .queue_click_preferences_command("1", setting("0", false))
        .unwrap();
    *api.playlist_tracks.lock().unwrap() = Some(vec![json!({"id":4}), json!({"id":5})]);
    api.hold_id.store(4, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        tokio::spawn(async move { client.play_selection("1", row("4")).await })
    };
    api.started.notified().await;
    let other = client.play_selection("1", songs("8")).await.unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        queue(&client).await,
        vec![("8".into(), other.entry_id.unwrap())]
    );
}

#[tokio::test]
async fn device_setting_reopens_fences_edits_and_damaged_state_cannot_clear_the_queue() {
    let (dir, api, session, client, _) = fixture();
    let view = serde_json::to_value(client.queue_click_preferences("1").unwrap()).unwrap();
    assert_eq!(
        view["value"],
        json!({"revision":"0","replaceQueue":true,"damaged":false})
    );
    client
        .queue_click_preferences_command("1", setting("0", false))
        .unwrap();
    assert!(
        client
            .queue_click_preferences_command("1", setting("0", true))
            .is_err()
    );
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let reopened = MusicClient::with_session(session.clone(), registry.clone());
    let view = serde_json::to_value(reopened.queue_click_preferences("1").unwrap()).unwrap();
    assert_eq!(
        view["value"],
        json!({"revision":"1","replaceQueue":false,"damaged":false})
    );

    let store = registry.store();
    store
        .put(
            "local-device",
            "playback-queue-click-preference-v1",
            "current",
            &json!({"broken":true}),
        )
        .unwrap();
    client.play_selection("1", songs("99")).await.unwrap();
    let before = queue(&client).await;
    let requests = api.calls.lock().unwrap().len();
    assert!(client.play_selection("1", row("2")).await.is_err());
    assert_eq!(queue(&client).await, before);
    assert_eq!(api.calls.lock().unwrap().len(), requests);
    let view = serde_json::to_value(reopened.queue_click_preferences("1").unwrap()).unwrap();
    assert_eq!(
        view["value"],
        json!({"revision":null,"replaceQueue":true,"damaged":true})
    );
    let repaired = reopened
        .queue_click_preferences_command("1", QueueClickPreferencesCommand::Repair)
        .unwrap();
    assert_eq!(
        serde_json::to_value(repaired).unwrap()["value"],
        json!({"revision":"2","replaceQueue":true,"damaged":false})
    );
    assert_eq!(
        store
            .values("local-device", "playback-queue-click-recovery-v1")
            .unwrap()
            .len(),
        1
    );
    session.detach().unwrap();
    assert!(
        client
            .queue_click_preferences_command("1", setting("2", false))
            .is_err()
    );
    let epoch = session.token().unwrap().generation.to_string();
    assert_eq!(
        serde_json::to_value(reopened.queue_click_preferences(&epoch).unwrap()).unwrap()["value"]["revision"],
        "2"
    );
}

#[test]
fn row_double_click_does_not_accept_explicit_or_excluded_selection_variants() {
    for selection in [
        json!({"kind":"songs","ids":["1"],"index":0,"quality":"standard"}),
        json!({"kind":"search_result_track","id":"1","quality":"standard","toggle_current":true}),
        json!({"kind":"playlist","id":"7","quality":"standard"}),
        json!({"kind":"playlist","id":"7","start_id":"1","quality":"standard","replace_queue":false}),
    ] {
        assert!(
            serde_json::from_value::<PlaySelection>(json!({
                "kind":"song_row_double_click","selection":selection
            }))
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<PlaySelection>(json!({
            "kind":"song_row_double_click",
            "selection":{"kind":"playlist","id":"7","start_id":"1","quality":"standard"}
        }))
        .is_ok()
    );
}
