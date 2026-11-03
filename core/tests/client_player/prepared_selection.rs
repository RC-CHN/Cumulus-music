use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

#[tokio::test]
async fn prepared_ticket_is_current_single_use_and_cannot_override_an_intervening_pause() {
    let (_dir, api, _session, client, _count) = fixture();
    let view = client.player_snapshot("1").await.unwrap();
    assert!(matches!(
        client.prepare_play_selection("1", "999", None),
        Err(Error::StaleResponse)
    ));
    let old = client
        .prepare_play_selection("1", &view.queue_revision, None)
        .unwrap();
    let current = client
        .prepare_play_selection("1", &view.queue_revision, None)
        .unwrap();
    assert!(matches!(
        client
            .commit_prepared_selection("1", &old, songs("1"))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(api.calls.lock().unwrap().is_empty());
    let playing = client
        .commit_prepared_selection("1", &current, songs("2"))
        .await
        .unwrap();
    assert_eq!(playing.track.as_ref().unwrap().id, "2");
    assert!(matches!(
        client
            .commit_prepared_selection("1", &current, songs("1"))
            .await,
        Err(Error::StaleResponse)
    ));
    let before_pause = client
        .prepare_play_selection("1", &playing.queue_revision, playing.play_id.as_deref())
        .unwrap();
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.play_id, playing.play_id);
    assert_eq!(paused.queue_revision, playing.queue_revision);
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .commit_prepared_selection("1", &before_pause, songs("1"))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), before);
}

#[tokio::test]
async fn prepared_selection_cannot_replace_a_newer_pending_native_intent() {
    let (_dir, api, _session, client, _count) = fixture();
    let view = client.player_snapshot("1").await.unwrap();
    let ticket = client
        .prepare_play_selection("1", &view.queue_revision, None)
        .unwrap();
    api.hold_id.store(1, Ordering::SeqCst);
    let newer = {
        let client = client.clone();
        tokio::spawn(async move { client.play_selection("1", songs("1")).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(4), api.started.notified())
        .await
        .unwrap();
    let before = api.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .commit_prepared_selection("1", &ticket, songs("2"))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), before);
    api.release.notify_one();
    let playing = newer.await.unwrap().unwrap();
    assert_eq!(playing.track.as_ref().unwrap().id, "1");
}

#[tokio::test]
async fn a_new_preparation_retires_an_older_ticket_while_native_metadata_is_pending() {
    let (_dir, api, _session, client, _count) = fixture();
    let view = client.player_snapshot("1").await.unwrap();
    let old = client
        .prepare_play_selection("1", &view.queue_revision, None)
        .unwrap();
    api.hold_id.store(1, Ordering::SeqCst);
    let older = {
        let client = client.clone();
        tokio::spawn(async move {
            client
                .commit_prepared_selection("1", &old, songs("1"))
                .await
        })
    };
    tokio::time::timeout(std::time::Duration::from_secs(4), api.started.notified())
        .await
        .unwrap();
    let current = client
        .prepare_play_selection("1", &view.queue_revision, None)
        .unwrap();
    api.release.notify_one();
    assert!(matches!(older.await.unwrap(), Err(Error::StaleResponse)));
    let idle = client.player_snapshot("1").await.unwrap();
    assert_eq!(idle.state, PlaybackStatus::Idle);
    assert!(idle.play_id.is_none());
    let playing = client
        .commit_prepared_selection("1", &current, songs("2"))
        .await
        .unwrap();
    assert_eq!(playing.track.as_ref().unwrap().id, "2");
}
