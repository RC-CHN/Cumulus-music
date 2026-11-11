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

fn search_menu_play(id: &str, state: &cumulus_core::client::dto::PlayerView) -> PlaySelection {
    PlaySelection::SearchResultMenuPlay {
        id: id.into(),
        quality: Quality::Standard,
        expected_play_id: state.play_id.clone(),
        expected_queue_revision: state.queue_revision.clone(),
    }
}

#[tokio::test]
async fn search_menu_play_moves_the_same_source_copy_and_current_play_only_resumes() {
    let (_dir, api, _, client, _) = fixture();
    api.playlist_trial.enable_media_trace();
    let initial = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "2", "3", "4"].map(str::to_owned).to_vec(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let original = queue(&client).await;
    client
        .player_action(
            "1",
            PlayerAction::SearchNextPlay {
                id: "3".into(),
                quality: Quality::Standard,
                expected_queue_revision: initial.queue_revision,
            },
        )
        .await
        .unwrap();
    let before = client
        .player_action(
            "1",
            PlayerAction::Select {
                entry_id: original[1].1.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        queue(&client).await,
        [0, 2, 1, 3].map(|index| original[index].clone())
    );

    // The old source is already search/42. This is an explicit add at the
    // current display slot, even though a generic row play can reuse the slot.
    let started = client
        .play_selection("1", search_menu_play("3", &before))
        .await
        .unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_ne!(started.play_id, before.play_id);
    assert_ne!(started.queue_revision, before.queue_revision);
    assert_eq!(started.entry_id.as_deref(), Some(original[2].1.as_str()));
    assert_eq!(queue(&client).await, original);
    let target_reads = api.playlist_trial.modes("3");
    assert!(!target_reads.is_empty());
    assert!(target_reads.iter().all(|mode| *mode == 42));

    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let detail_count = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.ends_with("/v3/song/detail"))
            .count()
    };
    let details_before = detail_count();
    let resumed = client
        .play_selection("1", search_menu_play("3", &paused))
        .await
        .unwrap();
    let already_playing = client
        .play_selection("1", search_menu_play("3", &resumed))
        .await
        .unwrap();
    assert_eq!(already_playing.state, PlaybackStatus::Playing);
    assert_eq!(already_playing.play_id, paused.play_id);
    assert_eq!(already_playing.queue_revision, paused.queue_revision);
    assert_eq!(queue(&client).await, original);
    assert_eq!(detail_count(), details_before);
    assert_eq!(api.playlist_trial.modes("3"), target_reads);
}

#[tokio::test]
async fn search_menu_play_rejects_stale_open_and_late_detail_without_touching_the_new_queue() {
    let (_dir, api, _, client, _) = fixture();
    let old = client.play_selection("1", songs("11")).await.unwrap();
    client.play_selection("1", songs("12")).await.unwrap();
    let initial_calls = api.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .play_selection("1", search_menu_play("31", &old))
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(
        !api.calls.lock().unwrap()[initial_calls..]
            .iter()
            .any(|path| path.ends_with("/v3/song/detail"))
    );

    for (id, superseding) in [(31, "13"), (32, "14")] {
        let before = client.player_snapshot("1").await.unwrap();
        api.hold_id.store(id, Ordering::SeqCst);
        api.search_menu_denied_detail.store(32, Ordering::SeqCst);
        let pending = {
            let client = client.clone();
            tokio::spawn(async move {
                client
                    .play_selection("1", search_menu_play(&id.to_string(), &before))
                    .await
            })
        };
        api.started.notified().await;
        let replacement = client
            .play_selection("1", songs(superseding))
            .await
            .unwrap();
        api.release.notify_one();
        assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
        let current = client.player_snapshot("1").await.unwrap();
        assert_eq!(current.play_id, replacement.play_id);
        assert_eq!(current.queue_revision, replacement.queue_revision);
        assert_eq!(
            queue(&client).await,
            [(superseding.into(), replacement.entry_id.unwrap())]
        );
    }
}

#[tokio::test]
async fn search_menu_play_rejects_confirmed_copyright_denial_before_replacing_the_paused_decoder() {
    let device = Arc::new(support::Device::default());
    let (_dir, api, _, client, _) = fixture_with_device(device.clone());
    api.playlist_trial.enable_media_trace();
    client.play_selection("1", songs("11")).await.unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before_queue = queue(&client).await;
    let before_loads = device.state.lock().unwrap().loads;
    api.search_menu_denied_detail.store(31, Ordering::SeqCst);

    let Err(error) = client
        .play_selection("1", search_menu_play("31", &paused))
        .await
    else {
        panic!("the copyright-restricted song was accepted");
    };
    assert!(matches!(
        error,
        Error::State("search menu song is unavailable")
    ));
    let public = cumulus_core::client::ClientError::from(error);
    assert_eq!(public.code, "search_menu_song_unavailable");
    assert_eq!(public.message, "因版权原因，该歌曲暂时无法播放");
    assert!(!public.retryable);

    let current = client.player_snapshot("1").await.unwrap();
    assert_eq!(current.state, PlaybackStatus::Paused);
    assert_eq!(current.entry_id, paused.entry_id);
    assert_eq!(current.play_id, paused.play_id);
    assert_eq!(current.queue_revision, paused.queue_revision);
    assert_eq!(current.position, paused.position);
    assert_eq!(queue(&client).await, before_queue);
    assert_eq!(device.state.lock().unwrap().loads, before_loads);
    assert!(api.playlist_trial.modes("31").is_empty());
}
