use super::*;
use cumulus_core::network_diagnostics::DefaultRoute;
const COUNT: &str = "/api/resource/commentInfo/list";

fn target(voice_id: &str, revision: &str) -> PodcastProgramTarget {
    PodcastProgramTarget {
        voice_id: voice_id.into(),
        radio_id: "7".into(),
        ascending: true,
        revision: revision.into(),
    }
}

#[tokio::test]
async fn program_menu_count_is_a_radio_bound_type_one_read_with_optional_plain_fallback() {
    let (_root, api, _, client) = fixture();
    let page = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    for invalid in [
        target("1", "old"),
        target("901", &page.revision),
        PodcastProgramTarget {
            ascending: false,
            ..target("1", &page.revision)
        },
        PodcastProgramTarget {
            radio_id: "8".into(),
            ..target("1", &page.revision)
        },
    ] {
        assert!(matches!(
            client
                .podcast_program_comment_count("1", invalid, DefaultRoute::Present)
                .await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(api.calls.lock().unwrap().is_empty());
    assert_eq!(
        client
            .podcast_program_comment_count("1", target("1", &page.revision), DefaultRoute::Missing)
            .await
            .unwrap()
            .value,
        None
    );
    assert!(api.calls.lock().unwrap().is_empty());
    assert_eq!(
        client
            .podcast_program_comment_count("1", target("1", &page.revision), DefaultRoute::Present)
            .await
            .unwrap()
            .value,
        Some(54)
    );
    assert_eq!(
        client
            .podcast_program_comment_count("1", target("2", &page.revision), DefaultRoute::Present)
            .await
            .unwrap()
            .value,
        None
    );
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            (
                COUNT.into(),
                json!({"resourceType":1,"resourceIds":"[\"1\"]"})
            ),
            (
                COUNT.into(),
                json!({"resourceType":1,"resourceIds":"[\"2\"]"})
            ),
        ]
    );
}

#[tokio::test]
async fn program_menu_count_cannot_return_after_replacing_radio_revision_or_account() {
    for account in [false, true] {
        let (_root, api, session, client) = fixture();
        let page = client
            .podcast_page("1", query(scope(), 0), true)
            .await
            .unwrap();
        api.menu_count_hold.store(true, Ordering::SeqCst);
        let read = client.podcast_program_comment_count(
            "1",
            target("1", &page.revision),
            DefaultRoute::Present,
        );
        let replace = async {
            api.menu_count_started.notified().await;
            if account {
                session
                    .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
                    .unwrap();
            } else {
                api.count.store(2, Ordering::SeqCst);
                let now = client
                    .podcast_page("1", query(scope(), 0), true)
                    .await
                    .unwrap();
                assert_ne!(now.revision, page.revision);
            }
            api.menu_count_release.notify_one();
        };
        let (reply, ()) = tokio::join!(read, replace);
        assert!(matches!(
            reply,
            Err(Error::StaleResponse | Error::QueueChanged)
        ));
    }
}

#[tokio::test]
async fn next_is_one_program_and_rejects_foreign_or_retired_radio_scope_before_episode_lookup() {
    let (_root, api, _, client) = fixture();
    let page = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    client
        .play_selection("1", PlaySelection::Voice { id: "1".into() })
        .await
        .unwrap();
    // Separate the explicit detail lookup from the existing next-source prefetch.
    let play = client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: cumulus_core::playback::PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    let command = |id: &str, selected| PlayerAction::QueueVoice {
        id: id.into(),
        program: Some(selected),
        expected_queue_revision: play.queue_revision.clone(),
    };
    for invalid in [
        command("2", target("1", &page.revision)),
        command("2", target("2", "retired")),
    ] {
        assert!(matches!(
            client.player_action("1", invalid).await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(api.calls.lock().unwrap().is_empty());
    let next = client
        .player_action("1", command("2", target("2", &page.revision)))
        .await
        .unwrap();
    assert_eq!(next.play_id, play.play_id);
    assert_eq!(next.entry_id, play.entry_id);
    assert_eq!(next.queue_len, 2);
    assert_ne!(next.queue_revision, play.queue_revision);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.voice_id.as_deref())
            .collect::<Vec<_>>(),
        [Some("1"), Some("2")]
    );
    let details = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == "/api/dj/program/detail")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        details,
        [("/api/dj/program/detail".into(), json!({"id":"2"}))]
    );
}
