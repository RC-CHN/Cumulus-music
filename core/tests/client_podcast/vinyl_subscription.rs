use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

const DETAIL: &str = "/api/djradio/v3/get";
const SUB: &str = "/api/djradio/sub";
const UNSUB: &str = "/api/djradio/unsub";

fn command(play: &PlayerView, expected: bool) -> PodcastCommand {
    // Exercise the exact generic desktop PodcastCommand JSON, not an alternate
    // native shape or an arbitrary renderer-supplied radio ID.
    serde_json::from_value(json!({
        "kind":"change_vinyl_subscription",
        "play_id":play.play_id,
        "expected":expected,
    }))
    .unwrap()
}

fn calls(api: &Backend) -> Vec<String> {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| matches!(path.as_str(), DETAIL | SUB | UNSUB))
        .map(|(path, body)| {
            assert_eq!(body, &json!({"id":"7"}));
            path.clone()
        })
        .collect()
}

async fn selection(client: &MusicClient, id: &str) -> PlayerView {
    let page = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    client
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: page.revision,
                start_id: Some(id.into()),
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn vinyl_subscription_reuses_journal_and_keeps_paused_native_identity() {
    let (_dir, api, _session, client, device) = fixture_with_device();
    api.count.store(2, Ordering::SeqCst);
    api.vinyl_fixture.store(true, Ordering::SeqCst);
    let current = selection(&client, "1").await;
    let card = current
        .voice_detail
        .as_ref()
        .unwrap()
        .radio
        .as_ref()
        .unwrap();
    assert_eq!(
        (&*card.id, card.subscriber_count, card.owned_by_viewer),
        ("7", Some(331), false)
    );
    assert!(
        serde_json::from_value::<PodcastCommand>(json!({
            "kind":"change_vinyl_subscription","play_id":current.play_id,"expected":false,"id":"8",
        }))
        .is_err()
    );
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let loads = device.state.lock().unwrap().loads;
    for (expected, write) in [(false, SUB), (true, UNSUB)] {
        api.calls.lock().unwrap().clear();
        let change = client
            .podcast_command("1", command(&paused, expected))
            .await
            .unwrap()
            .value;
        assert_eq!(
            (&*change.id, change.observed, change.acknowledged),
            ("7", Some(!expected), true)
        );
        assert_eq!(calls(&api), [DETAIL, write, DETAIL, DETAIL]);
        let after = client.player_snapshot("1").await.unwrap();
        assert_eq!(
            (
                &after.play_id,
                &after.entry_id,
                &after.queue_revision,
                after.position,
                after.state
            ),
            (
                &paused.play_id,
                &paused.entry_id,
                &paused.queue_revision,
                paused.position,
                paused.state
            )
        );
        assert_eq!(device.state.lock().unwrap().loads, loads);
    }
    let own = selection(&client, "2").await;
    assert!(
        own.voice_detail
            .as_ref()
            .unwrap()
            .radio
            .as_ref()
            .unwrap()
            .owned_by_viewer
    );
    api.calls.lock().unwrap().clear();
    assert!(matches!(
        client.podcast_command("1", command(&own, false)).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(calls(&api).is_empty());
}

#[tokio::test]
async fn vinyl_subscription_never_writes_after_playback_changes_during_detail() {
    let (_dir, api, _session, client) = fixture();
    api.count.store(1, Ordering::SeqCst);
    api.vinyl_fixture.store(true, Ordering::SeqCst);
    let before = selection(&client, "1").await;
    let client = Arc::new(client);
    api.calls.lock().unwrap().clear();
    api.vinyl_hold_detail.store(true, Ordering::SeqCst);
    let pending = {
        let client = client.clone();
        let command = command(&before, false);
        tokio::spawn(async move { client.podcast_command("1", command).await })
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        api.vinyl_detail_started.notified(),
    )
    .await
    .unwrap();
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    api.vinyl_detail_release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(calls(&api), [DETAIL]);
    assert!(!api.vinyl_subscribed.load(Ordering::SeqCst));
    assert!(
        client
            .podcast_state("1", false)
            .await
            .unwrap()
            .value
            .operations
            .is_empty()
    );
    assert!(matches!(
        client.podcast_command("1", command(&before, false)).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(calls(&api), [DETAIL]);
}
