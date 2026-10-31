use super::*;
use cumulus_core::{client::mini::MiniAction, playback::mode::ModeFamily};

// A drained durable outbox does not mean the background worker is idle: it
// can still be acquiring/releasing the room gate or applying a newer fence.
// Only retry these explicit pre-commit rejections, with a fresh local snapshot.
// Other failures and unknown remote write results are never retried here.
async fn mode_when_available(client: &MusicClient, mode: PlaybackMode) -> cumulus_core::Result<()> {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let shown = client.mini_snapshot()?;
            let result = client.mini_action(&shown, MiniAction::Mode { mode }).await;
            if result.as_ref().is_err_and(|error| {
                matches!(
                    error.root(),
                    Error::MutationBusy | Error::StaleResponse | Error::QueueChanged
                )
            }) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            } else {
                return result;
            }
        }
    })
    .await
    .map_err(|_| Error::State("mini controls did not become available"))?
}

#[tokio::test]
async fn mini_room_modes_use_the_attached_cycle_and_exit_revokes_old_controls() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api);
    play(&client).await;
    let ordinary = client.mini_snapshot().unwrap();
    assert_eq!(ordinary.view.mode_family, Some(ModeFamily::Ordinary));
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| {
        v.attached && v.pending.is_empty() && v.tracks.len() == 3
    })
    .await;
    assert!(matches!(
        client
            .mini_action(
                &ordinary,
                MiniAction::Mode {
                    mode: PlaybackMode::Sequential
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 18.0 })
        .await
        .unwrap();
    wait(&client, |v| {
        !v.playing && v.pending.is_empty() && v.progress_ms == Some(18000)
    })
    .await;
    let before = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    for expected in [
        PlaybackMode::SingleLoop,
        PlaybackMode::Random,
        PlaybackMode::OrderLoop,
    ] {
        let shown = client.mini_snapshot().unwrap();
        assert_eq!(shown.view.mode_family, Some(ModeFamily::Together));
        let denied = mode_when_available(&client, PlaybackMode::Sequential).await;
        assert!(
            matches!(
                denied.as_ref().map_err(|error| error.root()),
                Err(Error::InvalidInput(
                    "mode unavailable in mini playback context"
                ))
            ),
            "unexpected rejection: {denied:?}"
        );
        let shown = client.mini_snapshot().unwrap();
        let next = shown
            .view
            .mode_family
            .unwrap()
            .next(shown.view.mode.unwrap());
        assert_eq!(next, expected);
        mode_when_available(&client, next).await.unwrap();
        wait(&client, |v| v.pending.is_empty()).await;
        let current = client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap();
        assert_eq!(current.mode, expected);
        assert_eq!(current.play_id, before.play_id);
        assert_eq!(current.position, Some(18.0));
    }
    let room = client.mini_snapshot().unwrap();
    client
        .together_command(
            "1",
            TogetherCommand::End {
                room_id: "test-room".into(),
            },
        )
        .await
        .unwrap();
    let normal = client.mini_snapshot().unwrap();
    assert_eq!(normal.view.mode_family, Some(ModeFamily::Ordinary));
    assert!(!room.same_controls(&normal));
    assert!(matches!(
        client
            .mini_action(
                &room,
                MiniAction::Mode {
                    mode: PlaybackMode::SingleLoop
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn an_empty_outbox_can_still_hold_the_room_gate_and_reject_a_mini_mode_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let client = client.with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    incoming_tests::joined(&client, &api, &im).await;
    api.hold_privilege.store(true, Ordering::SeqCst);
    incoming_tests::send(&im, 20012, json!({"roomId":"test-room"}));
    tokio::time::timeout(Duration::from_secs(4), api.privilege_started.notified())
        .await
        .expect("held room read did not start");
    assert!(
        client
            .together_view("1", false)
            .await
            .unwrap()
            .value
            .pending
            .is_empty()
    );
    let shown = client.mini_snapshot().unwrap();
    assert_eq!(shown.view.mode_family, Some(ModeFamily::Together));
    let rejected = client
        .mini_action(
            &shown,
            MiniAction::Mode {
                mode: PlaybackMode::Sequential,
            },
        )
        .await;
    assert!(
        matches!(rejected, Err(Error::MutationBusy)),
        "busy gate must reject before mode policy: {rejected:?}"
    );
    let after = client.mini_snapshot().unwrap();
    assert!(shown.same_controls(&after));
    assert_eq!(shown.view.mode, after.view.mode);
    api.hold_privilege.store(false, Ordering::SeqCst);
    api.privilege_release.notify_one();
    wait(&client, |v| {
        v.shared_phase == Some(cumulus_core::listen_together::IncomingPhase::Applied)
    })
    .await;
    let rejected = mode_when_available(&client, PlaybackMode::Sequential).await;
    assert!(
        matches!(
            rejected.as_ref().map_err(|error| error.root()),
            Err(Error::InvalidInput(
                "mode unavailable in mini playback context"
            ))
        ),
        "available policy rejection: {rejected:?}"
    );
    assert_eq!(client.mini_snapshot().unwrap().view.mode, after.view.mode);
    client.close_together().await.unwrap();
}
