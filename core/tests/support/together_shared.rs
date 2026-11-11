use super::incoming_tests::{command, joined, send, writes};
use super::*;
use cumulus_core::{
    listen_together::{IncomingPhase, TogetherService},
    playback::{PlaybackQueue, controller::PlaybackStatus},
};
fn rights(id: &str, reason: &str) -> Value {
    json!({"songId":id,"playable":reason.is_empty(),"unplayableType":reason,"unplayableUserIds":if reason.is_empty(){json!([])}else{json!([2])},"unplayableOuterIds":[],"start":40,"end":50})
}
fn playlist() -> Value {
    json!({"displayList":{"result":["91","94","95"],"changed":true},"randomList":{"result":["95","91","94"],"changed":true},"version":[{"userId":1,"version":20}],"playMode":"RANDOM","replace":true})
}

#[tokio::test]
async fn peer_reports_stop_while_im_link_is_recovering_but_local_audio_remains_controllable() {
    use cumulus_core::listen_together::{native_im::ImEvent, transport::ConnectionState};
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    im.controls.lock().unwrap()[0]
        .send(ImEvent::Link {
            room: 31,
            condition: 1,
        })
        .unwrap();
    wait(&client, |v| {
        v.connection.state == ConnectionState::Reconnecting
    })
    .await;
    let before = writes(&api);
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&client, |v| v.sync_error.is_some()).await;
    assert_eq!(writes(&api), before);
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .state,
        PlaybackStatus::Paused
    );
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn incoming_queue_builds_new_songs_preserves_random_order_and_cleans_common_preview_on_exit()
{
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    let before = writes(&api);
    {
        let mut s = api.state.lock().unwrap();
        s.playlist = Some(playlist());
        s.play = Some(command(20, "PAUSE", "94", 47000, "PAUSE"));
    }
    *api.common.lock().unwrap() = Some(vec![
        rights("91", "vip"),
        rights("94", "trial"),
        rights("95", ""),
    ]);
    send(
        &im,
        20001,
        json!({"roomId":"test-room","version":[{"userId":1,"version":20}]}),
    );
    wait(&client, |v| {
        v.shared_phase == Some(IncomingPhase::Applied) && v.tracks.iter().any(|t| t.id == "94")
    })
    .await;
    let player = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(player.track.unwrap().id, "94");
    assert_eq!(
        client
            .together_view("1", false)
            .await
            .unwrap()
            .value
            .native_target_id
            .as_deref(),
        Some("94")
    );
    assert_eq!(player.position, Some(47.0));
    assert_eq!(player.state, PlaybackStatus::Paused);
    assert_eq!(
        audio.state.lock().unwrap().bounds.last(),
        Some(&(40.0, Some(50.0)))
    );
    let queue = PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(root.path()).unwrap(),
        )),
        "1",
    )
    .unwrap()
    .snapshot();
    let order = queue
        .order
        .iter()
        .map(|id| {
            queue
                .items
                .iter()
                .find(|e| &e.entry_id == id)
                .unwrap()
                .resource["id"]
                .as_str()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(order, vec!["95", "91", "94"]);
    assert_eq!(writes(&api), before);
    client.player_action("1", PlayerAction::Next).await.unwrap();
    wait(&client, |v| {
        v.target_id.as_deref() == Some("95") && v.pending.is_empty()
    })
    .await;
    client.close_together().await.unwrap();
    let queue = PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(root.path()).unwrap(),
        )),
        "1",
    )
    .unwrap()
    .snapshot();
    assert!(
        queue
            .items
            .iter()
            .all(|e| e.resource.get("playback_constraint").is_none()
                && e.resource.get("shared_room").is_none())
    );
}

#[tokio::test]
async fn same_version_privilege_change_restricts_current_audio_and_all_blocked_stops_without_reports()
 {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    let service = TogetherService::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    let version = service
        .state_for(&token)
        .unwrap()
        .snapshot
        .playlist
        .unwrap()
        .version;
    let before = writes(&api);
    let reads = || {
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("playlist/get"))
            .count()
    };
    let read_before = reads();
    *api.common.lock().unwrap() = Some(vec![
        rights("91", "trial"),
        rights("92", ""),
        rights("93", ""),
    ]);
    send(&im, 20001, json!({"roomId":"test-room","version":version}));
    wait(&client, |v| {
        v.shared_phase == Some(IncomingPhase::Applied)
            && v.common_access
                .iter()
                .any(|p| p.track_id == "91" && p.preview.is_some())
    })
    .await;
    let player = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(player.position, Some(40.0));
    assert_eq!(player.timeline.unwrap().end, Some(50.0));
    assert_eq!(
        reads(),
        read_before,
        "equal vector must refresh rights without refetching the list"
    );
    *api.common.lock().unwrap() = Some(vec![
        rights("91", "vip"),
        rights("92", "vip"),
        rights("93", "vip"),
    ]);
    send(&im, 20001, json!({"roomId":"test-room","version":version}));
    wait(&client, |v| {
        v.shared_phase == Some(IncomingPhase::Applied)
            && v.common_access.len() == 3
            && v.common_access.iter().all(|p| !p.can_play)
    })
    .await;
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .state,
        PlaybackStatus::Idle
    );
    assert!(client.player_action("1", PlayerAction::Play).await.is_err());
    assert_eq!(writes(&api), before);
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn pause_at_zero_survives_redundant_shared_read_but_changed_rights_stay_blocked() {
    enum PriorPlay {
        Applied,
        IgnoredLocal,
        IgnoredPeer,
    }
    use PriorPlay::*;
    for (prior_play, changed_rights) in [
        (Applied, false),
        (Applied, true),
        (IgnoredLocal, false),
        (IgnoredPeer, false),
    ] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(Backend::default());
        let (session, client) = world(root.path(), api.clone());
        let im = Arc::new(im::Device::default());
        let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
        play(&client).await;
        joined(&client, &api, &im).await;
        let service = TogetherService::new(session.clone()).unwrap();
        let token = session.token().unwrap();
        wait(&client, |v| {
            v.playing
                && v.pending.is_empty()
                && service
                    .state_for(&token)
                    .unwrap()
                    .shared
                    .is_some_and(|r| r.phase == IncomingPhase::Applied && !r.ignore_play_command)
        })
        .await;
        if !matches!(prior_play, Applied) {
            let prior = service.state_for(&token).unwrap().shared.unwrap().id;
            let version = service
                .state_for(&token)
                .unwrap()
                .snapshot
                .playlist
                .unwrap()
                .version;
            if matches!(prior_play, IgnoredPeer) {
                api.state.lock().unwrap().play =
                    Some(command(17, "PROGRESS", "91", 17_000, "PLAY"));
                // Equal-version LIST reads the latest cached list and command.
                // The read updates that cache without applying native transport.
                client.together_view("1", true).await.unwrap();
            }
            send(&im, 20001, json!({"roomId":"test-room","version":version}));
            wait(&client, |_| {
                service.state_for(&token).unwrap().shared.is_some_and(|r| {
                    r.id != prior && r.ignore_play_command && r.phase == IncomingPhase::Applied
                })
            })
            .await;
            let ignored = service.state_for(&token).unwrap().shared.unwrap();
            let play = ignored.snapshot.unwrap().play_command.unwrap();
            match prior_play {
                IgnoredLocal => assert_eq!(play.user_id.as_deref(), Some("1")),
                IgnoredPeer => assert_eq!(play.user_id.as_deref(), Some("2")),
                Applied => unreachable!(),
            }
        }
        let before = client.player_snapshot("1").await.unwrap();
        assert_eq!(before.state, PlaybackStatus::Playing);
        assert_eq!(before.position, Some(0.0));
        let reports = || {
            api.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path == "/api/listen/together/play/command/report")
                .map(|(_, body)| {
                    serde_json::from_str::<Value>(body["commandInfo"].as_str().unwrap()).unwrap()
                })
                .collect::<Vec<_>>()
        };
        let count = reports().len();
        if changed_rights {
            *api.common.lock().unwrap() = Some(vec![
                rights("91", "trial"),
                rights("92", ""),
                rights("93", ""),
            ]);
        }
        api.hold_privilege.store(true, Ordering::SeqCst);
        send(&im, 20002, json!({"roomId":"test-room"}));
        tokio::time::timeout(Duration::from_secs(4), api.privilege_started.notified())
            .await
            .unwrap();
        let paused = client
            .player_action("1", PlayerAction::Pause { paused: true })
            .await
            .unwrap();
        assert_eq!(paused.state, PlaybackStatus::Paused);
        assert_eq!(paused.position, Some(0.0));
        assert_eq!(paused.play_id, before.play_id);
        assert_eq!(reports().len(), count);
        api.hold_privilege.store(false, Ordering::SeqCst);
        api.privilege_release.notify_one();
        if changed_rights || matches!(prior_play, IgnoredPeer) {
            wait(&client, |v| {
                v.shared_phase == Some(IncomingPhase::Superseded) && v.sync_error.is_some()
            })
            .await;
            assert_eq!(reports().len(), count);
        } else {
            wait(&client, |v| {
                v.shared_phase == Some(IncomingPhase::Applied)
                    && !v.playing
                    && v.pending.is_empty()
                    && v.sync_error.is_none()
            })
            .await;
            let wire = reports();
            assert_eq!(wire.len(), count + 1);
            assert_eq!(wire[count]["commandType"], "PAUSE");
            assert_eq!(wire[count]["playStatus"], "PAUSE");
            assert_eq!(wire[count]["progress"], 0);
            assert_eq!(wire[count]["targetSongId"], "91");
        }
        let after = client.player_snapshot("1").await.unwrap();
        assert_eq!(after.play_id, before.play_id);
        assert_eq!(after.queue_revision, before.queue_revision);
        assert_eq!(after.position, Some(0.0));
        assert_eq!(after.state, PlaybackStatus::Paused);
        if matches!(prior_play, IgnoredPeer) {
            client
                .together_command(
                    "1",
                    TogetherCommand::Publish {
                        room_id: "test-room".into(),
                    },
                )
                .await
                .unwrap();
            wait(&client, |v| {
                v.shared_phase == Some(IncomingPhase::Applied)
                    && v.playing
                    && v.pending.is_empty()
                    && v.sync_error.is_none()
            })
            .await;
            let recovered = client.player_snapshot("1").await.unwrap();
            assert_eq!(recovered.state, PlaybackStatus::Playing);
            assert_eq!(recovered.position, Some(17.0));
            assert_eq!(
                reports().len(),
                count,
                "read-only recovery must not echo peer transport"
            );
        }
        client.close_together().await.unwrap();
    }
}

#[tokio::test]
async fn local_seek_during_shared_rights_fetch_prevents_late_queue_install() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_session, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    api.state.lock().unwrap().playlist = Some(playlist());
    api.hold_privilege.store(true, Ordering::SeqCst);
    send(
        &im,
        20001,
        json!({"roomId":"test-room","version":[{"userId":1,"version":20}]}),
    );
    tokio::time::timeout(Duration::from_secs(4), api.privilege_started.notified())
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 66.0 })
        .await
        .unwrap();
    api.hold_privilege.store(false, Ordering::SeqCst);
    api.privilege_release.notify_one();
    wait(&client, |v| {
        v.shared_phase == Some(IncomingPhase::Superseded)
    })
    .await;
    let current = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(current.position, Some(66.0));
    assert_eq!(current.track.unwrap().id, "91");
    let queue = PlaybackQueue::new(
        Some(Arc::new(
            cumulus_core::storage::StateStore::open(root.path()).unwrap(),
        )),
        "1",
    )
    .unwrap()
    .snapshot();
    assert!(!queue.items.iter().any(|e| e.resource["id"] == "94"));
    client.close_together().await.unwrap();
}
