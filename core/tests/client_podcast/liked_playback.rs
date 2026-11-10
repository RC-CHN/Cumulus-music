use super::*;
use cumulus_core::client::ClientError;
use cumulus_core::network_diagnostics::DefaultRoute;

fn card(id: u64) -> Value {
    json!({"voiceId":id,"voiceName":format!("Liked {id}"),"likeTime":1234})
}

fn selection(revision: &str, start_id: Option<&str>) -> PlaySelection {
    PlaySelection::Podcast {
        scope: PodcastScope::Liked,
        revision: revision.into(),
        start_id: start_id.map(str::to_owned),
    }
}

fn menu_target(revision: &str, id: &str) -> PodcastLikedProgramTarget {
    PodcastLikedProgramTarget {
        revision: revision.into(),
        voice_id: id.into(),
    }
}

async fn menu_play(
    client: &MusicClient,
    target: PodcastLikedProgramTarget,
) -> cumulus_core::Result<PlayerView> {
    let current = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &current.queue_revision, current.play_id.as_deref())?;
    client
        .commit_prepared_selection("1", &ticket, PlaySelection::PodcastLikedEpisode { target })
        .await
}

#[tokio::test]
async fn liked_playback_retains_masked_cards_but_queues_only_verified_media_in_original_order() {
    let (_dir, api, _session, client) = fixture();
    let mut withdrawn = program(501);
    withdrawn["displayStatus"] = json!("UNDER_SHELF");
    let mut normal = program(504);
    normal["displayStatus"] = json!("NORMAL");
    *api.liked_cards.lock().unwrap() =
        Some(vec![withdrawn, card(502), card(503), normal, card(505)]);
    *api.liked_details.lock().unwrap() = [
        (502, json!({"code":404})),
        (503, json!({"code":200,"program":program(503)})),
        (505, json!({"code":200,"program":card(505)})),
    ]
    .into();
    let page = client
        .podcast_page("1", query(PodcastScope::Liked, 0), true)
        .await
        .unwrap();
    assert_eq!(
        page.episodes
            .iter()
            .map(|v| v.id.as_str())
            .collect::<Vec<_>>(),
        ["501", "502", "503", "504", "505"]
    );
    assert_eq!(page.liked_ids, ["501", "502", "503", "504", "505"]);
    assert!(page.episodes[0].unavailable);
    assert_eq!(page.episodes[0].withdrawn, Some(true));
    assert!(page.episodes[3].withdrawn.is_none());
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: cumulus_core::playback::PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    let playing = client
        .play_selection("1", selection(&page.revision, Some("504")))
        .await
        .unwrap();
    assert_eq!(playing.track.unwrap().voice_id.as_deref(), Some("504"));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|entry| entry.track.voice_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["503", "504"]
    );
    assert_eq!(
        queue.current.as_deref(),
        Some(queue.tracks[1].entry_id.as_str())
    );
    {
        let calls = api.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|(path, _)| path == "/api/dj/program/detail")
                .map(|(_, p)| p["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["502", "503", "505", "504"]
        );
        assert!(
            calls
                .iter()
                .any(|(path, p)| path == "/api/dj/play/url/check" && p["voiceId"] == "504")
        );
        assert!(
            !calls
                .iter()
                .any(|(path, p)| path == "/api/dj/play/url/check"
                    && matches!(p["voiceId"].as_str(), Some("501" | "502" | "505")))
        );
    }
    for (id, expected) in [
        ("501", "liked_voice_withdrawn"),
        ("502", "liked_voice_unavailable"),
    ] {
        api.calls.lock().unwrap().clear();
        let error = client
            .play_selection("1", selection(&page.revision, Some(id)))
            .await
            .err()
            .unwrap();
        assert_eq!(ClientError::from(error).code, expected);
        let current = client.player_snapshot("1").await.unwrap();
        assert_eq!(
            (current.play_id, current.queue_revision),
            (playing.play_id.clone(), queue.revision.clone())
        );
        let calls = api.calls.lock().unwrap();
        assert!(!calls.iter().any(|(path, _)| matches!(
            path.as_str(),
            "/api/dj/play/url/check" | "/api/song/enhance/player/url/v1"
        )));
        let details = calls
            .iter()
            .filter(|(path, _)| path == "/api/dj/program/detail")
            .map(|(_, body)| body["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(details, if id == "501" { vec![] } else { vec!["502"] });
    }
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn liked_playback_invalidates_withdrawal_changes_and_never_ignores_unknown_details() {
    let (_dir, api, _session, client) = fixture();
    *api.liked_cards.lock().unwrap() = Some(vec![program(501), card(502)]);
    let page = client
        .podcast_page("1", query(PodcastScope::Liked, 0), true)
        .await
        .unwrap();
    api.liked_cards.lock().unwrap().as_mut().unwrap()[0]["displayStatus"] = json!("BLOCKED");
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        client
            .play_selection("1", selection(&page.revision, Some("501")))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        ["/api/content/my/liked/voice"]
    );
    let updated = client
        .podcast_page("1", query(PodcastScope::Liked, 0), false)
        .await
        .unwrap();
    assert_ne!(updated.revision, page.revision);
    api.liked_details
        .lock()
        .unwrap()
        .insert(502, json!({"code":200,"program":program(999)}));
    assert!(matches!(
        client
            .play_selection("1", selection(&updated.revision, None))
            .await,
        Err(Error::Response("program detail identity mismatch"))
    ));
    api.liked_details
        .lock()
        .unwrap()
        .insert(502, json!({"code":404}));
    let error = client
        .play_selection("1", selection(&updated.revision, None))
        .await
        .err()
        .unwrap();
    assert_eq!(ClientError::from(error).code, "liked_voice_queue_empty");
    assert!(client.player_queue("1").await.unwrap().tracks.is_empty());
}

#[tokio::test]
async fn liked_menu_is_single_program_play_and_next_without_replacing_a_paused_incumbent() {
    let (_dir, api, _session, client) = fixture();
    let mut withdrawn = program(501);
    withdrawn["displayStatus"] = json!("REMOVED");
    *api.liked_cards.lock().unwrap() =
        Some(vec![withdrawn, program(503), program(504), program(505)]);
    let page = client
        .podcast_page("1", query(PodcastScope::Liked, 0), true)
        .await
        .unwrap();
    let selected = menu_target(&page.revision, "504");
    let unavailable = menu_target(&page.revision, "501");
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        client
            .podcast_liked_program_menu("1", &unavailable, DefaultRoute::Present)
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client
            .download_liked_voice_prepare("1", unavailable.clone())
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: cumulus_core::playback::PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    let error = menu_play(&client, unavailable).await.err().unwrap();
    assert!(matches!(error, Error::QueueChanged), "{error:?}");
    let menu = serde_json::to_value(
        client
            .podcast_liked_program_menu("1", &selected, DefaultRoute::Present)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        menu["value"],
        json!({"commentCount":54,"downloadComplete":false})
    );
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap(),
        &(
            "/api/resource/commentInfo/list".into(),
            json!({"resourceType":1,"resourceIds":"[\"504\"]"})
        )
    );
    let first = menu_play(&client, selected.clone()).await.unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.voice_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["504"]
    );
    let still = menu_play(&client, selected.clone()).await.unwrap();
    assert_eq!(
        (still.play_id, still.state, still.queue_revision),
        (first.play_id.clone(), first.state, queue.revision)
    );
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let next = client
        .player_action(
            "1",
            PlayerAction::QueueLikedProgram {
                target: menu_target(&page.revision, "503"),
                expected_queue_revision: paused.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (next.play_id, next.entry_id, next.position, next.state),
        (
            paused.play_id,
            paused.entry_id,
            paused.position,
            paused.state
        )
    );
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.voice_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["504", "503"]
    );
    let resumed = menu_play(&client, selected).await.unwrap();
    assert_eq!(resumed.play_id, first.play_id);
    assert_eq!(resumed.queue_revision, queue.revision);
    assert_eq!(resumed.state, first.state);
    client.player_action("1", PlayerAction::Stop).await.unwrap();
}

#[tokio::test]
async fn liked_menu_drops_a_held_count_after_withdrawal_and_rejects_old_file_authority() {
    let (_dir, api, _session, client) = fixture();
    *api.liked_cards.lock().unwrap() = Some(vec![program(503)]);
    let page = client
        .podcast_page("1", query(PodcastScope::Liked, 0), true)
        .await
        .unwrap();
    let target = menu_target(&page.revision, "503");
    api.menu_count_hold.store(true, Ordering::SeqCst);
    let (result, _) = tokio::join!(
        client.podcast_liked_program_menu("1", &target, DefaultRoute::Present),
        async {
            api.menu_count_started.notified().await;
            api.liked_cards.lock().unwrap().as_mut().unwrap()[0]["displayStatus"] =
                json!("UNDER_SHELF");
            let new = client
                .podcast_page("1", query(PodcastScope::Liked, 0), true)
                .await
                .unwrap();
            assert_ne!(new.revision, page.revision);
            api.menu_count_release.notify_one();
        }
    );
    assert!(matches!(result, Err(Error::QueueChanged)));
    assert!(matches!(
        client
            .podcast_liked_program_directory_path("1", &target)
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client.podcast_liked_program("1", &target),
        Err(Error::QueueChanged)
    ));
}
