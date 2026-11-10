use super::*;
#[path = "../support/queue_voice_publication.rs"]
mod queue_publication;
use cumulus_core::network_diagnostics::DefaultRoute;
const COUNT: &str = "/api/resource/commentInfo/list";

#[tokio::test]
async fn queue_voice_menu_reads_the_selected_episode_and_discards_a_retired_row() {
    let (_root, api, _, client) = fixture();
    api.count.store(3, Ordering::SeqCst);
    let page = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    let active = client
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: page.revision,
                start_id: Some("1".into()),
            },
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 3);
    assert!(
        queue
            .tracks
            .iter()
            .all(|row| row.voice_menu && !row.catalog_menu)
    );
    let second = &queue.tracks[1].entry_id;
    let third = &queue.tracks[2].entry_id;
    let read = |revision, entry, route| client.player_queue_voice_menu("1", revision, entry, route);
    let count_calls = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == COUNT)
            .map(|(_, args)| args.clone())
            .collect::<Vec<_>>()
    };
    assert!(matches!(
        read("0", third, DefaultRoute::Present).await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        read(&queue.revision, "3", DefaultRoute::Present).await,
        Err(Error::InvalidInput(_))
    ));
    let offline = read(&queue.revision, third, DefaultRoute::Missing)
        .await
        .unwrap();
    assert_eq!(offline.comment_count, None);
    assert!(count_calls().is_empty());
    let menu = read(&queue.revision, third, DefaultRoute::Present)
        .await
        .unwrap();
    assert_eq!(
        (menu.track.id.as_str(), menu.voice_id.as_str()),
        ("1003", "3")
    );
    assert_eq!(menu.comment_count, Some(54));
    assert_eq!(menu.source_label, "Radio");
    assert_eq!(menu.radio_id.as_deref(), Some("7"));
    assert_eq!(menu.creator.as_ref().unwrap().name, "Host");
    assert_eq!(menu.downloaded, Some(false));
    assert!(!menu.directory_available);
    assert!(!serde_json::to_string(&menu).unwrap().contains("SECRET"));
    let plain = read(&queue.revision, second, DefaultRoute::Present)
        .await
        .unwrap();
    assert_eq!(plain.comment_count, None);
    assert!(matches!(
        client
            .player_queue_voice_file_reveal("1", &queue.revision, third)
            .await,
        Err(Error::DownloadedFileMissing)
    ));

    api.menu_count_hold.store(true, Ordering::SeqCst);
    let pending = read(&queue.revision, third, DefaultRoute::Present);
    let edit = async {
        api.menu_count_started.notified().await;
        let current = client
            .player_action(
                "1",
                PlayerAction::QueueFor {
                    expected_queue_revision: queue.revision.clone(),
                    expected_room_id: None,
                    edit: QueueEdit::Remove {
                        entry_ids: vec![third.clone()],
                    },
                },
            )
            .await
            .unwrap();
        assert_eq!(current.play_id, active.play_id);
        assert_eq!(current.entry_id, active.entry_id);
        assert_eq!(current.queue_len, 2);
        api.menu_count_release.notify_one();
    };
    let (retired, ()) = tokio::join!(pending, edit);
    assert!(matches!(retired, Err(Error::QueueChanged)));
    assert_eq!(
        count_calls(),
        [
            json!({"resourceType":1,"resourceIds":"[\"3\"]"}),
            json!({"resourceType":1,"resourceIds":"[\"2\"]"}),
            json!({"resourceType":1,"resourceIds":"[\"3\"]"}),
        ]
    );
}

#[tokio::test]
async fn queue_voice_reveal_selects_only_the_unique_live_publication_without_playing_it() {
    let (root, api, _, client) = fixture();
    api.count.store(2, Ordering::SeqCst);
    let page = client
        .podcast_page("1", query(scope(), 0), true)
        .await
        .unwrap();
    let playing = client
        .play_selection(
            "1",
            PlaySelection::Podcast {
                scope: scope(),
                revision: page.revision,
                start_id: Some("1".into()),
            },
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    let entry = &queue.tracks[1].entry_id;
    let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let parent = root.path().join("test media");
    std::fs::create_dir(&parent).unwrap();
    let expected = parent.join("second episode.flac");
    let wrong = parent.join("other main audio.flac");
    queue_publication::publish(store.clone(), 9, "2", "9999", wrong)
        .await
        .unwrap();
    let resolve = || client.player_queue_voice_file_reveal("1", &queue.revision, entry);
    assert!(matches!(resolve().await, Err(Error::DownloadedFileMissing)));
    queue_publication::publish(store.clone(), 9, "2", "1002", expected.clone())
        .await
        .unwrap();
    let reveal = resolve().await.unwrap();
    assert_eq!(reveal.path(), expected.canonicalize().unwrap());
    reveal.check_current(&client).await.unwrap();
    let state = client.player_snapshot("1").await.unwrap();
    assert_eq!(state.play_id, playing.play_id);
    assert_eq!(state.queue_revision, queue.revision);
    let other = parent.join("same episode alternate.flac");
    queue_publication::publish(store, 9, "2", "1002", other.clone())
        .await
        .unwrap();
    assert!(matches!(
        resolve().await,
        Err(Error::State("program has multiple completed files"))
    ));
    assert!(reveal.check_current(&client).await.is_err());
    std::fs::remove_file(other).unwrap();
    reveal.check_current(&client).await.unwrap();
    let held = expected.with_extension("held");
    std::fs::rename(&expected, &held).unwrap();
    std::fs::write(&expected, queue_publication::AUDIO).unwrap();
    assert!(reveal.check_current(&client).await.is_err());
    std::fs::remove_file(&expected).unwrap();
    std::fs::rename(&held, &expected).unwrap();
    let live = resolve().await.unwrap();
    client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_queue_revision: queue.revision.clone(),
                expected_room_id: None,
                edit: QueueEdit::Remove {
                    entry_ids: vec![entry.clone()],
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        live.check_current(&client).await,
        Err(Error::QueueChanged)
    ));
    assert!(expected.is_file());
}

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

async fn recent_revision(client: &MusicClient, query: PodcastRecentQuery) -> String {
    serde_json::to_value(client.podcast_recent("1", query).await.unwrap().value).unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn recent_target(id: &str, revision: &str) -> PodcastRecentProgramTarget {
    PodcastRecentProgramTarget {
        voice_id: id.into(),
        revision: revision.into(),
    }
}

#[tokio::test]
async fn recent_program_menu_rejects_held_reply_after_pagination_and_rechecks_action_target() {
    let (_root, api, session, client) = fixture();
    let revision = recent_revision(&client, PodcastRecentQuery::Open).await;
    assert!(matches!(
        client.podcast_recent_program("1", recent_target("1042", &revision)),
        Err(Error::QueueChanged)
    ));
    api.menu_count_hold.store(true, Ordering::SeqCst);
    let read = client.podcast_recent_program_menu(
        "1",
        recent_target("42", &revision),
        DefaultRoute::Present,
    );
    let paginate = async {
        api.menu_count_started.notified().await;
        let next = recent_revision(
            &client,
            PodcastRecentQuery::Next {
                revision: revision.clone(),
            },
        )
        .await;
        api.menu_count_release.notify_one();
        next
    };
    let (old, current) = tokio::join!(read, paginate);
    assert!(matches!(old, Err(Error::QueueChanged)));
    assert!(matches!(
        client.podcast_recent_program("1", recent_target("42", &revision)),
        Err(Error::QueueChanged)
    ));
    let resolved = client
        .podcast_recent_program("1", recent_target("44", &current))
        .unwrap();
    assert_eq!(resolved.value.id, "44");
    assert_eq!(resolved.value.radio_id.as_deref(), Some("7"));
    assert!(!serde_json::to_string(&resolved).unwrap().contains("SECRET"));
    let menu = client
        .podcast_recent_program_menu("1", recent_target("44", &current), DefaultRoute::Present)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(menu.value).unwrap(),
        json!({"commentCount":54,"downloadComplete":false})
    );
    let calls_before_offline = api.calls.lock().unwrap().len();
    let menu = client
        .podcast_recent_program_menu("1", recent_target("44", &current), DefaultRoute::Missing)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(menu.value).unwrap(),
        json!({"commentCount":null,"downloadComplete":false})
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls_before_offline);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == COUNT)
            .map(|(_, body)| body.clone())
            .collect::<Vec<_>>(),
        [
            json!({"resourceType":1,"resourceIds":"[\"42\"]"}),
            json!({"resourceType":1,"resourceIds":"[\"44\"]"}),
        ]
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client.podcast_recent_program("1", recent_target("44", &current)),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.podcast_recent_program("2", recent_target("44", &current)),
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn recent_program_menu_next_cancels_held_hydration_then_keeps_paused_source_and_play_is_single()
 {
    let (_root, api, _, client) = fixture();
    api.recent_missing_media.store(true, Ordering::SeqCst);
    let revision = recent_revision(&client, PodcastRecentQuery::Open).await;
    client
        .play_selection("1", PlaySelection::Voice { id: "1".into() })
        .await
        .unwrap();
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: cumulus_core::playback::PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    api.calls.lock().unwrap().clear();
    let next = |target| PlayerAction::QueueRecentProgram {
        target,
        expected_queue_revision: paused.queue_revision.clone(),
    };
    api.recent_detail_hold.store(true, Ordering::SeqCst);
    let pending = client.player_action("1", next(recent_target("43", &revision)));
    let refresh = async {
        api.recent_detail_started.notified().await;
        let fresh = recent_revision(&client, PodcastRecentQuery::Refresh).await;
        api.recent_detail_release.notify_one();
        fresh
    };
    let (stale, fresh) = tokio::join!(pending, refresh);
    assert!(matches!(stale, Err(Error::QueueChanged)));
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(after.queue_revision, paused.queue_revision);
    assert_eq!(after.play_id, paused.play_id);
    let current = client
        .player_action("1", next(recent_target("43", &fresh)))
        .await
        .unwrap();
    assert_eq!(current.play_id, paused.play_id);
    assert_eq!(current.entry_id, paused.entry_id);
    assert_eq!(current.position, paused.position);
    assert_eq!(current.state, paused.state);
    assert_eq!(current.queue_len, 2);
    assert_ne!(current.queue_revision, paused.queue_revision);
    assert_eq!(
        client
            .player_queue("1")
            .await
            .unwrap()
            .tracks
            .iter()
            .map(|row| row.track.voice_id.as_deref())
            .collect::<Vec<_>>(),
        [Some("1"), Some("43")]
    );
    assert!(matches!(
        client
            .player_action("1", next(recent_target("42", &fresh)))
            .await,
        Err(Error::QueueChanged)
    ));
    let played = client
        .play_selection(
            "1",
            PlaySelection::PodcastRecent {
                revision: fresh,
                id: "42".into(),
                double_click: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(played.track.unwrap().voice_id.as_deref(), Some("42"));
    assert_ne!(played.play_id, paused.play_id);
    assert_eq!(
        client
            .player_queue("1")
            .await
            .unwrap()
            .tracks
            .iter()
            .map(|row| row.track.voice_id.as_deref())
            .collect::<Vec<_>>(),
        [Some("1"), Some("42"), Some("43")]
    );
    let trace = api.calls.lock().unwrap();
    assert_eq!(
        trace
            .iter()
            .filter(|(path, _)| path == "/api/dj/program/detail")
            .map(|(_, body)| body["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["43", "43", "42"]
    );
    assert!(!trace.iter().any(|(path, _)| path.contains("/byradio")));
}
