use super::*;
use cumulus_core::playback::{PlaybackMode, controller::PlaybackStatus};
use std::time::Duration;

fn setup_pages(api: &Backend) {
    let mut withdrawn = program(502);
    withdrawn["displayStatus"] = json!("UNDER_SHELF");
    let mut last = program(504);
    last["likeTime"] = json!("440");
    *api.liked_page_replies.lock().unwrap() = vec![
        (
            json!(0),
            json!({"code":200,"data":{"data":[program(501),withdrawn,
                {"voiceId":503,"voiceName":"Missing media"},last],"hasMore":true}}),
        ),
        (
            json!("440"),
            json!({"code":200,"data":{"data":[program(505),program(506)],"hasMore":false}}),
        ),
    ];
}

async fn open(client: &MusicClient) -> PodcastLikedPage {
    let page = client
        .podcast_liked_page("1", PodcastLikedQuery::Open {})
        .await
        .unwrap()
        .value;
    next(client, &page).await
}

async fn next(client: &MusicClient, page: &PodcastLikedPage) -> PodcastLikedPage {
    client
        .podcast_liked_page(
            "1",
            PodcastLikedQuery::Next {
                visit: page.visit.clone(),
                revision: page.revision.clone(),
                continuation: page.continuation.clone().unwrap(),
            },
        )
        .await
        .unwrap()
        .value
}

fn selection(page: &PodcastLikedPage, voice_id: &str, double_click: bool) -> PlaySelection {
    PlaySelection::PodcastLikedPageEpisode {
        target: target(page, voice_id),
        double_click,
    }
}

fn target(page: &PodcastLikedPage, voice_id: &str) -> PodcastLikedPageTarget {
    PodcastLikedPageTarget {
        visit: page.visit.clone(),
        revision: page.revision.clone(),
        voice_id: voice_id.into(),
    }
}

fn queue_next(page: &PodcastLikedPage, voice_id: &str, expected: &str) -> PlayerAction {
    PlayerAction::QueueLikedPageProgram {
        target: target(page, voice_id),
        expected_queue_revision: expected.into(),
    }
}

async fn select(
    client: &MusicClient,
    selection: PlaySelection,
) -> cumulus_core::Result<PlayerView> {
    let state = client.player_snapshot("1").await?;
    let ticket =
        client.prepare_play_selection("1", &state.queue_revision, state.play_id.as_deref())?;
    client
        .commit_prepared_selection("1", &ticket, selection)
        .await
}

async fn queue(client: &MusicClient) -> Vec<String> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|row| row.track.voice_id.unwrap())
        .collect()
}

async fn incumbent(client: &MusicClient) -> PlayerView {
    // A one-entry looping queue would also authorize its own prefetch;
    // use the ordinary sequential endpoint for this isolated incumbent.
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await
        .unwrap();
    client
        .play_selection("1", PlaySelection::Voice { id: "900".into() })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap()
}

#[tokio::test]
async fn paged_liked_menu_and_double_click_use_only_the_loaded_prefix_and_preference() {
    let (_root, api, _session, client) = fixture();
    setup_pages(&api);
    let page = open(&client).await;
    assert_eq!(page.loaded_count, 4);
    incumbent(&client).await;
    api.calls.lock().unwrap().clear();
    let played = select(&client, selection(&page, "504", false))
        .await
        .unwrap();
    assert_eq!(queue(&client).await, ["900", "504"]);
    let same = select(&client, selection(&page, "504", false))
        .await
        .unwrap();
    assert_eq!(same.play_id, played.play_id);
    assert_eq!(same.state, PlaybackStatus::Playing);
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    let toggled = select(&client, selection(&page, "504", true))
        .await
        .unwrap();
    assert_eq!(toggled.play_id, played.play_id);
    assert_eq!(toggled.state, PlaybackStatus::Paused);
    assert_eq!(toggled.mode, PlaybackMode::Sequential);
    assert_eq!(queue(&client).await, ["900", "504"]);
    let resumed = select(&client, selection(&page, "504", false))
        .await
        .unwrap();
    assert_eq!(resumed.play_id, played.play_id);
    assert_eq!(resumed.state, PlaybackStatus::Playing);

    let replaced = select(&client, selection(&page, "501", true))
        .await
        .unwrap();
    assert_eq!(replaced.track.unwrap().voice_id.as_deref(), Some("501"));
    assert_eq!(queue(&client).await, ["501", "504"]);
    assert!(matches!(
        select(&client, selection(&page, "502", true)).await,
        Err(Error::State("liked voice withdrawn"))
    ));
    assert!(matches!(
        select(&client, selection(&page, "506", false)).await,
        Err(Error::QueueChanged)
    ));
    {
        let calls = api.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|(path, body)| {
                path == "/api/content/my/liked/voice"
                    || (path == "/api/dj/program/detail" && body["id"] == "503")
                    || path == "/api/dj/around/program/v1"
            }),
            "unexpected continuation or sibling hydration: {calls:?}"
        );
    }

    let last = next(&client, &page).await;
    client
        .queue_click_preferences_command(
            "1",
            QueueClickPreferencesCommand::Change {
                expected_revision: "0".into(),
                replace_queue: false,
            },
        )
        .unwrap();
    let appended = select(&client, selection(&last, "506", true))
        .await
        .unwrap();
    assert_eq!(appended.track.unwrap().voice_id.as_deref(), Some("506"));
    assert_eq!(queue(&client).await, ["501", "506", "504"]);
}

#[tokio::test]
async fn paged_liked_retirement_during_authorization_preserves_the_incumbent() {
    let (_root, api, _session, client) = fixture();
    setup_pages(&api);
    let page = open(&client).await;
    let before = incumbent(&client).await;
    api.calls.lock().unwrap().clear();
    api.search_playback_hold.store(true, Ordering::SeqCst);
    let (result, restored) = tokio::join!(select(&client, selection(&page, "501", true)), async {
        tokio::time::timeout(
            Duration::from_secs(5),
            api.search_playback_started.notified(),
        )
        .await
        .unwrap();
        client
            .podcast_liked_page(
                "1",
                PodcastLikedQuery::Suspend {
                    visit: page.visit.clone(),
                    revision: page.revision.clone(),
                },
            )
            .await
            .unwrap();
        let restored = client
            .podcast_liked_page(
                "1",
                PodcastLikedQuery::Restore {
                    visit: page.visit.clone(),
                    revision: page.revision.clone(),
                },
            )
            .await
            .unwrap()
            .value;
        api.search_playback_release.notify_one();
        restored
    });
    assert!(matches!(
        result,
        Err(Error::Cancelled | Error::QueueChanged | Error::StaleResponse)
    ));
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(
        (after.play_id, after.queue_revision, after.state),
        (before.play_id, before.queue_revision, before.state)
    );
    assert_eq!(queue(&client).await, ["900"]);
    assert!(matches!(
        select(&client, selection(&page, "501", false)).await,
        Err(Error::QueueChanged)
    ));
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, body)| path == "/api/song/enhance/player/url/v1"
                && body["ids"] == r#"["1501"]"#)
    );
    let valid = select(&client, selection(&restored, "501", false))
        .await
        .unwrap();
    assert_eq!(valid.track.unwrap().voice_id.as_deref(), Some("501"));
}

#[tokio::test]
async fn paged_liked_hydrates_the_clicked_missing_media_before_permission_and_commit() {
    let (_root, api, _session, client) = fixture();
    setup_pages(&api);
    api.liked_details
        .lock()
        .unwrap()
        .insert(503, json!({"code":200,"program":program(503)}));
    let page = open(&client).await;
    let before = incumbent(&client).await;
    api.calls.lock().unwrap().clear();
    api.search_playback_deny.store(503, Ordering::SeqCst);
    assert!(matches!(
        select(&client, selection(&page, "503", true)).await,
        Err(Error::State("program playback entitlement unavailable"))
    ));
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(queue(&client).await, ["900"]);
    assert_eq!(
        *api.calls.lock().unwrap(),
        [
            ("/api/dj/program/detail".into(), json!({"id":"503"})),
            ("/api/dj/play/url/check".into(), json!({"voiceId":"503"})),
        ]
    );
    api.search_playback_deny.store(0, Ordering::SeqCst);
    let played = select(&client, selection(&page, "503", true))
        .await
        .unwrap();
    assert_eq!(played.track.unwrap().voice_id.as_deref(), Some("503"));
    assert_eq!(queue(&client).await, ["501", "503", "504"]);
}

#[tokio::test]
async fn paged_liked_next_inserts_only_the_row_and_keeps_the_paused_incumbent() {
    let (_root, api, _session, client, device) = fixture_with_device();
    setup_pages(&api);
    let page = open(&client).await;
    let before = incumbent(&client).await;
    api.calls.lock().unwrap().clear();
    for (id, revision) in [("506", before.queue_revision.as_str()), ("504", "invalid")] {
        assert!(matches!(
            client
                .player_action("1", queue_next(&page, id, revision))
                .await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(matches!(
        client
            .player_action("1", queue_next(&page, "502", &before.queue_revision))
            .await,
        Err(Error::State("liked voice withdrawn"))
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    let old_loads = device.state.lock().unwrap().loads;
    let after = client
        .player_action("1", queue_next(&page, "504", &before.queue_revision))
        .await
        .unwrap();
    assert_eq!(queue(&client).await, ["900", "504"]);
    let inserted = client
        .player_action("1", queue_next(&page, "501", &after.queue_revision))
        .await
        .unwrap();
    assert_eq!(queue(&client).await, ["900", "501", "504"]);
    assert_eq!(
        (
            inserted.play_id,
            inserted.entry_id,
            inserted.position,
            inserted.state,
            inserted.mode
        ),
        (
            before.play_id,
            before.entry_id,
            before.position,
            PlaybackStatus::Paused,
            before.mode
        )
    );
    assert_eq!(device.state.lock().unwrap().loads, old_loads);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/api/content/my/liked/voice")
    );
}

#[tokio::test]
async fn paged_liked_next_on_empty_queue_authorizes_only_the_clicked_episode() {
    let (_root, api, _session, client) = fixture();
    setup_pages(&api);
    let page = open(&client).await;
    let before = client.player_snapshot("1").await.unwrap();
    api.calls.lock().unwrap().clear();
    api.search_playback_deny.store(504, Ordering::SeqCst);
    assert!(matches!(
        client
            .player_action("1", queue_next(&page, "504", &before.queue_revision))
            .await,
        Err(Error::State("program playback entitlement unavailable"))
    ));
    assert!(queue(&client).await.is_empty());
    assert_eq!(
        client.player_snapshot("1").await.unwrap().queue_revision,
        before.queue_revision
    );
    assert_eq!(
        *api.calls.lock().unwrap(),
        [("/api/dj/play/url/check".into(), json!({"voiceId":"504"}))]
    );
    api.search_playback_deny.store(0, Ordering::SeqCst);
    let after = client
        .player_action("1", queue_next(&page, "504", &before.queue_revision))
        .await
        .unwrap();
    assert_eq!(queue(&client).await, ["504"]);
    assert_eq!(after.track.unwrap().voice_id.as_deref(), Some("504"));
    assert_eq!(after.state, PlaybackStatus::Playing);
    assert_eq!(after.position, Some(15.0));
    assert_eq!(after.mode, before.mode);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/api/content/my/liked/voice")
    );
}

#[tokio::test]
async fn paged_liked_next_during_incumbent_load_does_not_restart_it() {
    let (_root, api, _session, client, device) = fixture_with_device();
    setup_pages(&api);
    let page = open(&client).await;
    device.hold_load.store(true, Ordering::SeqCst);
    let (played, opening) = tokio::join!(
        client.play_selection("1", PlaySelection::Voice { id: "900".into() }),
        async {
            tokio::time::timeout(Duration::from_secs(5), device.started.notified())
                .await
                .unwrap();
            let before = client.player_snapshot("1").await.unwrap();
            let after = tokio::time::timeout(
                Duration::from_secs(5),
                client.player_action("1", queue_next(&page, "504", &before.queue_revision)),
            )
            .await;
            device.hold_load.store(false, Ordering::SeqCst);
            let after = after
                .expect("queue-only Next must return before load completes")
                .unwrap();
            assert!(matches!(
                before.state,
                PlaybackStatus::Preparing | PlaybackStatus::Loading
            ));
            assert_eq!(
                (after.entry_id, after.play_id.as_ref(), after.state),
                (before.entry_id, before.play_id.as_ref(), before.state)
            );
            before.play_id
        }
    );
    let played = played.unwrap();
    assert_eq!(played.play_id, opening);
    assert_eq!(played.track.unwrap().voice_id.as_deref(), Some("900"));
    assert_eq!(device.state.lock().unwrap().loads, 1);
    assert_eq!(queue(&client).await, ["900", "504"]);
}

#[tokio::test]
async fn paged_liked_next_discards_hydration_after_a_new_visit() {
    let (_root, api, _session, client) = fixture();
    setup_pages(&api);
    let page = open(&client).await;
    let before = incumbent(&client).await;
    api.calls.lock().unwrap().clear();
    api.liked_page_detail_hold.store(true, Ordering::SeqCst);
    let (result, _) = tokio::join!(
        client.player_action("1", queue_next(&page, "503", &before.queue_revision)),
        async {
            tokio::time::timeout(
                Duration::from_secs(5),
                api.search_playback_started.notified(),
            )
            .await
            .unwrap();
            client
                .podcast_liked_page("1", PodcastLikedQuery::Open {})
                .await
                .unwrap();
            api.search_playback_release.notify_one();
        }
    );
    assert!(matches!(
        result,
        Err(Error::Cancelled | Error::QueueChanged | Error::StaleResponse)
    ));
    assert_eq!(queue(&client).await, ["900"]);
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(
        (after.play_id, after.queue_revision, after.state),
        (before.play_id, before.queue_revision, before.state)
    );
    assert_eq!(
        *api.calls.lock().unwrap(),
        [("/api/dj/program/detail".into(), json!({"id":"503"}))]
    );
}
