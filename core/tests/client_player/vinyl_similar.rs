use super::*;
use cumulus_core::{
    client::{VinylRecommendationPlacement as Placement, VinylSimilarAction},
    playback::controller::PlaybackStatus,
    protocol::api::VinylSimilarSongsRead,
};

#[derive(Default)]
pub(super) struct Server {
    calls: Mutex<Vec<Value>>,
    response: Mutex<Option<Value>>,
    held: AtomicU64,
    started: Notify,
    release: Notify,
}

impl Server {
    pub(super) async fn response(
        &self,
        read: VinylSimilarSongsRead,
    ) -> cumulus_core::Result<Value> {
        let (path, args) = read.into_request();
        assert_eq!(path, "/api/v1/discovery/simiSong");
        self.calls.lock().unwrap().push(args);
        if self.held.swap(0, Ordering::SeqCst) != 0 {
            self.started.notify_one();
            self.release.notified().await;
        }
        Ok(self.response.lock().unwrap().clone().unwrap_or_else(|| {
            json!({"code":200,"songs":[
                {"id":4,"name":"相似甲","dt":120000,"privilege":{"pl":128000},"authorization":"NOT_PUBLIC"},
                {"id":5,"name":"相似乙","dt":120000,"privilege":{"pl":128000}},
                {"id":6,"name":"不可用","dt":120000,"privilege":{"pl":0}},
            ]})
        }))
    }
}

async fn seed_middle(client: &MusicClient) -> String {
    client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into(), "3".into()],
                index: 1,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap()
        .play_id
        .unwrap()
}

async fn queue_ids(client: &MusicClient) -> Vec<String> {
    client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|row| row.track.id)
        .collect()
}

#[tokio::test]
async fn vinyl_similar_treatment_wire_and_separate_header_cover_queue_actions() {
    let (_dir, api, _, client, _) = fixture();
    let play_id = seed_middle(&client).await;
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Unresolved
    );
    assert!(client.player_vinyl_similar_available("1").await.is_err());
    assert!(
        client
            .player_vinyl_similar_songs("1", &play_id)
            .await
            .is_err()
    );
    assert!(api.vinyl_similar.calls.lock().unwrap().is_empty());

    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    assert!(client.player_vinyl_similar_available("1").await.unwrap());
    let view = client
        .player_vinyl_similar_songs("1", &play_id)
        .await
        .unwrap();
    assert_eq!(view.play_id, play_id);
    assert_eq!(view.song_id, "2");
    assert_eq!(view.placement, Placement::Standalone);
    assert_eq!(
        view.songs
            .iter()
            .map(|row| (row.track.id.as_str(), row.playable))
            .collect::<Vec<_>>(),
        [("4", true), ("5", true), ("6", false)]
    );
    assert!(!serde_json::to_string(&view).unwrap().contains("NOT_PUBLIC"));
    let receipt = view.receipt.unwrap();
    let before = queue_ids(&client).await;
    for (candidate, index) in [
        ("0123456789abcdef0123456789abcdef", 0),
        (&receipt, 2),
        (&receipt, 3),
    ] {
        assert!(
            client
                .player_vinyl_similar_play(
                    "1",
                    &play_id,
                    candidate,
                    VinylSimilarAction::Cover { index },
                    Quality::Standard
                )
                .await
                .is_err()
        );
    }
    assert_eq!(queue_ids(&client).await, before);

    let header = client
        .player_vinyl_similar_play(
            "1",
            &play_id,
            &receipt,
            VinylSimilarAction::Header,
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(header.track.unwrap().id, "4");
    assert_eq!(queue_ids(&client).await, ["1", "2", "4", "5", "3"]);
    assert!(matches!(
        client
            .player_vinyl_similar_play(
                "1",
                &play_id,
                &receipt,
                VinylSimilarAction::Header,
                Quality::Standard
            )
            .await,
        Err(Error::StaleResponse)
    ));

    let play_id = seed_middle(&client).await;
    let page = client
        .player_vinyl_similar_songs("1", &play_id)
        .await
        .unwrap();
    let cover = client
        .player_vinyl_similar_play(
            "1",
            &play_id,
            &page.receipt.unwrap(),
            VinylSimilarAction::Cover { index: 1 },
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(cover.track.unwrap().id, "5");
    assert_eq!(queue_ids(&client).await, ["1", "2", "5", "3"]);
    assert_eq!(
        *api.vinyl_similar.calls.lock().unwrap(),
        [json!({"songid":"2"}), json!({"songid":"2"})]
    );
}

#[tokio::test]
async fn vinyl_verified_wiki_uses_both_native_lanes_and_the_full_initial_song_list() {
    let (_dir, api, _, client, _) = fixture();
    let play_id = seed_middle(&client).await;
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Wiki
    );
    assert!(
        !client.player_vinyl_similar_available("1").await.unwrap(),
        "old IPC remains standalone-only"
    );
    let playlists = client
        .player_vinyl_related_playlists("1", &play_id)
        .await
        .unwrap();
    assert_eq!(playlists.placement, Placement::Wiki);
    assert_eq!(playlists.song_id, "2");
    assert_eq!(
        playlists.playlists.len(),
        8,
        "expansion is not a different wire read"
    );

    *api.vinyl_similar.response.lock().unwrap() = Some(json!({
        "code":200,"songs":(4..=8).map(|id| json!({"id":id,"name":"相似歌曲","dt":120000,"privilege":{"pl":128000}})).collect::<Vec<_>>()
    }));
    let page = client
        .player_vinyl_similar_songs("1", &play_id)
        .await
        .unwrap();
    assert_eq!(page.placement, Placement::Wiki);
    assert_eq!(
        page.songs.len(),
        5,
        "control must not invent a three-row default"
    );
    let player = client
        .player_vinyl_similar_play(
            "1",
            &play_id,
            &page.receipt.unwrap(),
            VinylSimilarAction::Header,
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(player.track.unwrap().id, "4");
    assert_eq!(
        queue_ids(&client).await,
        ["1", "2", "4", "5", "6", "7", "8", "3"]
    );
    assert_eq!(
        *api.vinyl_similar.calls.lock().unwrap(),
        [json!({"songid":"2"})]
    );
}

#[tokio::test]
async fn vinyl_unresolved_or_blank_assignment_cannot_read_either_card() {
    for group in [4, 5] {
        let (_dir, api, _, client, _) = fixture();
        api.vinyl_comment_group.store(1, Ordering::SeqCst);
        api.vinyl_recommendation_group
            .store(group, Ordering::SeqCst);
        let play_id = seed_middle(&client).await;
        assert_eq!(
            client
                .player_vinyl_recommendation_placement("1")
                .await
                .unwrap(),
            Placement::Unresolved
        );
        assert!(matches!(
            client.player_vinyl_similar_songs("1", &play_id).await,
            Err(Error::State(_))
        ));
        assert!(matches!(
            client.player_vinyl_related_playlists("1", &play_id).await,
            Err(Error::State(_))
        ));
        assert!(api.vinyl_similar.calls.lock().unwrap().is_empty());
        assert_eq!(queue_ids(&client).await, ["1", "2", "3"]);
    }
}

#[tokio::test(start_paused = true)]
async fn vinyl_receipt_tracks_observed_placement_even_when_the_credential_revision_does_not_change()
{
    use std::time::Duration;
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    let play = seed_middle(&client).await;
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Wiki
    );
    tokio::time::advance(Duration::from_secs(29 * 60)).await;
    let old = client
        .player_vinyl_similar_songs("1", &play)
        .await
        .unwrap()
        .receipt
        .unwrap();

    api.vinyl_recommendation_group.store(3, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Wiki
    );
    // A different explicit non-t1 group still has the same Wiki placement.
    client
        .player_vinyl_similar_play(
            "1",
            &play,
            &old,
            VinylSimilarAction::Cover { index: 1 },
            Quality::Standard,
        )
        .await
        .unwrap();

    let play = seed_middle(&client).await;
    tokio::time::advance(Duration::from_secs(29 * 60)).await;
    let old = client
        .player_vinyl_similar_songs("1", &play)
        .await
        .unwrap()
        .receipt
        .unwrap();
    api.vinyl_recommendation_group.store(1, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Standalone
    );
    assert_eq!(
        api.verified_pc_experiment_revision().unwrap().as_deref(),
        Some("2")
    );
    assert!(matches!(
        client
            .player_vinyl_similar_play(
                "1",
                &play,
                &old,
                VinylSimilarAction::Header,
                Quality::Standard
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(queue_ids(&client).await, ["1", "2", "3"]);
    let fresh = client.player_vinyl_similar_songs("1", &play).await.unwrap();
    assert_eq!(fresh.placement, Placement::Standalone);
    assert_ne!(fresh.receipt.as_deref(), Some(old.as_str()));
}

#[tokio::test(start_paused = true)]
async fn vinyl_delayed_song_read_cannot_publish_a_receipt_for_the_replaced_placement() {
    use std::time::Duration;
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    let play = seed_middle(&client).await;
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Wiki
    );
    tokio::time::advance(Duration::from_secs(29 * 60)).await;
    api.vinyl_similar.held.store(1, Ordering::SeqCst);
    let read = tokio::spawn({
        let client = client.clone();
        let play = play.clone();
        async move { client.player_vinyl_similar_songs("1", &play).await }
    });
    api.vinyl_similar.started.notified().await;
    api.vinyl_recommendation_group.store(1, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(
        client
            .player_vinyl_recommendation_placement("1")
            .await
            .unwrap(),
        Placement::Standalone
    );
    api.vinyl_similar.release.notify_one();
    assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(queue_ids(&client).await, ["1", "2", "3"]);
    let fresh = client.player_vinyl_similar_songs("1", &play).await.unwrap();
    assert_eq!(fresh.placement, Placement::Standalone);
    assert!(fresh.receipt.is_some());
}

#[tokio::test]
async fn vinyl_similar_double_click_uses_device_queue_policy_and_current_transport() {
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    for replace in [true, false] {
        if !replace {
            client
                .queue_click_preferences_command(
                    "1",
                    QueueClickPreferencesCommand::Change {
                        expected_revision: "0".into(),
                        replace_queue: false,
                    },
                )
                .unwrap();
        }
        let play_id = seed_middle(&client).await;
        let receipt = client
            .player_vinyl_similar_songs("1", &play_id)
            .await
            .unwrap()
            .receipt
            .unwrap();
        let result = client
            .player_vinyl_similar_play(
                "1",
                &play_id,
                &receipt,
                VinylSimilarAction::DoubleClick { index: 1 },
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_eq!(result.track.unwrap().id, "5");
        let expected: &[&str] = if replace {
            &["4", "5"]
        } else {
            &["1", "2", "5", "3"]
        };
        assert_eq!(queue_ids(&client).await, expected);
        let current = result.play_id.unwrap();
        let receipt = client
            .player_vinyl_similar_songs("1", &current)
            .await
            .unwrap()
            .receipt
            .unwrap();
        let pause = client
            .player_vinyl_similar_play(
                "1",
                &current,
                &receipt,
                VinylSimilarAction::DoubleClick { index: 1 },
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_eq!(pause.play_id.as_deref(), Some(current.as_str()));
        assert!(pause.state == PlaybackStatus::Paused || pause.pending_paused == Some(true));
        let resume = client
            .player_vinyl_similar_play(
                "1",
                &current,
                &receipt,
                VinylSimilarAction::DoubleClick { index: 1 },
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_eq!(resume.play_id.as_deref(), Some(current.as_str()));
        assert_ne!(resume.state, PlaybackStatus::Paused);
        assert_ne!(resume.pending_paused, Some(true));
        assert_eq!(queue_ids(&client).await, expected);
    }
}

#[tokio::test]
async fn vinyl_similar_stale_play_revision_account_and_empty_or_denied_results_stay_closed() {
    let (_dir, api, session, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    for change in ["track", "credentials", "account"] {
        let play = seed_middle(&client).await;
        let old = client
            .player_vinyl_similar_songs("1", &play)
            .await
            .unwrap()
            .receipt
            .unwrap();
        api.vinyl_similar.held.store(1, Ordering::SeqCst);
        let read = tokio::spawn({
            let client = client.clone();
            let play = play.clone();
            async move { client.player_vinyl_similar_songs("1", &play).await }
        });
        api.vinyl_similar.started.notified().await;
        match change {
            "track" => {
                client.play_selection("1", songs("7")).await.unwrap();
            }
            "credentials" => api.vinyl_comment_group.store(3, Ordering::SeqCst),
            "account" => {
                session
                    .adopt(api.clone(), json!({"userId":19}), &session.token().unwrap())
                    .unwrap();
            }
            _ => unreachable!(),
        }
        api.vinyl_similar.release.notify_one();
        assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
        assert!(matches!(
            client
                .player_vinyl_similar_play(
                    "1",
                    &play,
                    &old,
                    VinylSimilarAction::Header,
                    Quality::Standard
                )
                .await,
            Err(Error::StaleResponse)
        ));

        if change == "credentials" {
            for songs in [
                json!([]),
                json!([
                    {"id":8,"name":"首首无权","privilege":{"pl":0}},
                    {"id":9,"name":"不该跳到下一首","privilege":{"pl":128000}}
                ]),
            ] {
                *api.vinyl_similar.response.lock().unwrap() =
                    Some(json!({"code":200,"songs":songs}));
                let page = client.player_vinyl_similar_songs("1", &play).await.unwrap();
                if let Some(receipt) = page.receipt {
                    assert_eq!(page.songs.len(), 2);
                    assert!(matches!(
                        client
                            .player_vinyl_similar_play(
                                "1",
                                &play,
                                &receipt,
                                VinylSimilarAction::Header,
                                Quality::Standard
                            )
                            .await,
                        Err(Error::State(_))
                    ));
                } else {
                    assert!(page.songs.is_empty());
                }
                assert_eq!(queue_ids(&client).await, ["1", "2", "3"]);
            }
            *api.vinyl_similar.response.lock().unwrap() = None;
        }
    }
}

#[tokio::test]
async fn vinyl_similar_current_first_header_and_current_artwork_keep_the_decoder() {
    let device = Arc::new(support::Device::default());
    let (_dir, api, _, client, _) = fixture_with_device(device.clone());
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let play_id = seed_middle(&client).await;
    let positioned = client
        .player_action("1", PlayerAction::Seek { seconds: 22.0 })
        .await
        .unwrap();
    assert_eq!(positioned.state, PlaybackStatus::Playing);
    assert_eq!(positioned.position, Some(22.0));
    let entry_id = positioned.entry_id.unwrap();
    let engine_entry = device.state.lock().unwrap().view.entry_id;
    assert!(engine_entry.is_some());
    // Both existing generic UpdateQueue paths would otherwise be tempting:
    // rebinding the entry reloads; using remote transport with the same
    // entry still seeks. Neither is allowed for the current-first case.
    device.fail_seek.store(true, Ordering::SeqCst);

    for (paused, alternatives, expected) in [
        (false, [4, 5], &["1", "2", "4", "5", "3"][..]),
        (true, [6, 4], &["1", "2", "6", "4", "5", "3"][..]),
    ] {
        if paused {
            client
                .player_action("1", PlayerAction::Pause { paused: true })
                .await
                .unwrap();
        }
        let recommended = [2, alternatives[0], alternatives[1]].map(|id| {
            json!({"id":id,"name":"来自推荐的不同元数据","dt":120000,"privilege":{"pl":128000}})
        });
        *api.vinyl_similar.response.lock().unwrap() = Some(json!({"code":200,"songs":recommended}));
        let view = client
            .player_vinyl_similar_songs("1", &play_id)
            .await
            .unwrap();
        let receipt = view.receipt.unwrap();
        let pause_offset = device.state.lock().unwrap().pauses.len();
        let header = client
            .player_vinyl_similar_play(
                "1",
                &play_id,
                &receipt,
                VinylSimilarAction::Header,
                Quality::Lossless,
            )
            .await
            .unwrap();
        assert_eq!(header.play_id.as_deref(), Some(play_id.as_str()));
        assert_eq!(header.entry_id.as_deref(), Some(entry_id.as_str()));
        assert_eq!(header.position, Some(22.0));
        assert_eq!(header.state, PlaybackStatus::Playing);
        assert_eq!(queue_ids(&client).await, expected);
        {
            let device = device.state.lock().unwrap();
            assert_eq!(device.loads, 1);
            assert_eq!(device.view.entry_id, engine_entry);
            assert_eq!(
                &device.pauses[pause_offset..],
                if paused { &[false][..] } else { &[] }
            );
        }

        let cover_pause_offset = device.state.lock().unwrap().pauses.len();
        let cover = client
            .player_vinyl_similar_play(
                "1",
                &play_id,
                &receipt,
                VinylSimilarAction::Cover { index: 0 },
                Quality::Standard,
            )
            .await
            .unwrap();
        assert_eq!(cover.play_id, header.play_id);
        assert_eq!(cover.entry_id, header.entry_id);
        assert_eq!(cover.position, header.position);
        assert_eq!(cover.state, PlaybackStatus::Playing);
        assert_eq!(queue_ids(&client).await, expected);
        assert_eq!(
            device.state.lock().unwrap().pauses.len(),
            cover_pause_offset
        );
    }

    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let receipt = client
        .player_vinyl_similar_songs("1", &play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    let resumed = client
        .player_vinyl_similar_play(
            "1",
            &play_id,
            &receipt,
            VinylSimilarAction::Cover { index: 0 },
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(resumed.play_id, paused.play_id);
    assert_eq!(resumed.entry_id, paused.entry_id);
    assert_eq!(resumed.position, paused.position);
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    let native = device.state.lock().unwrap();
    assert_eq!(native.loads, 1);
    assert_eq!(native.view.entry_id, engine_entry);
    assert!(native.pauses.ends_with(&[true, false]));
}
