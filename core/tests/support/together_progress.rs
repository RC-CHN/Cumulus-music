use super::incoming_tests::{command, joined, send, writes};
use super::*;
use cumulus_core::listen_together::{native_im::ImEvent, transport::ConnectionState};

async fn position(client: &MusicClient, expected: f64) {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if client
                .subscribe_playback("1")
                .await
                .unwrap()
                .current()
                .unwrap()
                .position
                == Some(expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("native clock did not advance");
}
fn last_report(api: &Backend) -> Value {
    let calls = api.calls.lock().unwrap();
    let (_, value) = calls
        .iter()
        .rev()
        .find(|(path, _)| path == "/api/listen/together/play/command/report")
        .unwrap();
    serde_json::from_str(value["commandInfo"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn progress_requests_use_current_native_clock_and_ignore_numeric_account_only() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| {
        v.connection.state == ConnectionState::Connected
            && v.target_id.is_some()
            && v.pending.is_empty()
    })
    .await;
    let before = writes(&api);
    let queue_before = client.player_queue("1").await.unwrap();
    audio.state.lock().unwrap().view.position = Some(23.456);
    position(&client, 23.456).await;
    for content in [
        json!({"roomId":"test-room"}),
        json!({"roomId":"test-room","ignoreUserIds":[1]}),
        json!({"roomId":"different-room","ignoreUserIds":[]}),
    ] {
        send(&im, 20008, content);
    }
    // This sentinel follows the ignored requests on the same SDK channel.
    send(
        &im,
        20008,
        json!({"roomId":"test-room","ignoreUserIds":["1"]}),
    );
    wait(&client, |v| {
        v.progress_ms == Some(23456) && v.pending.is_empty()
    })
    .await;
    assert_eq!(writes(&api), before + 1);
    let report = last_report(&api);
    assert_eq!(report["commandType"], "PROGRESS");
    assert_eq!(report["playStatus"], "PLAY");
    assert_eq!(report["formerSongId"], "91");
    assert_eq!(report["targetSongId"], "91");
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        queue_before.revision
    );
    // Duplicate filtering has a one-second wall-clock window. Its exact
    // boundary is tested with explicit timestamps in together_protocol;
    // asynchronous database/SDK work here may legitimately outlast it.
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn progress_request_does_not_retry_lost_acknowledgement_or_report_during_reconnection() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    let before = writes(&api);
    audio.state.lock().unwrap().view.position = Some(34.567);
    position(&client, 34.567).await;
    api.drop_play.store(true, Ordering::SeqCst);
    send(&im, 20008, json!({"roomId":"test-room","ignoreUserIds":[]}));
    wait(&client, |v| !v.pending.is_empty() && v.sync_error.is_some()).await;
    assert_eq!(writes(&api), before + 1);
    assert_eq!(last_report(&api)["progress"], 34567);
    let reads = api.calls.lock().unwrap().len();
    send(&im, 20008, json!({"roomId":"test-room","ignoreUserIds":[]}));
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        api.calls.lock().unwrap().len(),
        reads,
        "request must not become explicit recovery"
    );
    client.close_together().await.unwrap();
    // A fresh room avoids using an unresolved write as the disconnected guard.
    api.state.lock().unwrap().peer = false;
    joined(&client, &api, &im).await;
    im.controls
        .lock()
        .unwrap()
        .last()
        .unwrap()
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
    send(&im, 20008, json!({"roomId":"test-room","ignoreUserIds":[]}));
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(writes(&api), before);
    client.close_together().await.unwrap();
}

#[tokio::test]
async fn common_preview_progress_reports_full_song_time_and_preserves_pause() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    let im = Arc::new(im::Device::default());
    let audio = Arc::new(engine::Device::default());
    let output = audio.clone();
    let client = client
        .with_audio_factory(move || Ok(engine::audio_engine(output.clone())))
        .with_together_im_factory(Arc::new(im::Factory(im.clone())));
    play(&client).await;
    joined(&client, &api, &im).await;
    api.state.lock().unwrap().play = Some(command(200, "PAUSE", "91", 44000, "PAUSE"));
    *api.common.lock().unwrap() = Some(["91", "92", "93"].map(|id| json!({
        "songId":id,"playable":id!="91","unplayableType":if id=="91" {"trial"} else {""},
        "unplayableUserIds":[2],"unplayableOuterIds":[],"start":40,"end":50,
    })).to_vec());
    send(&im, 20012, json!({"roomId":"test-room"}));
    wait(&client, |v| {
        v.common_access
            .iter()
            .any(|p| p.track_id == "91" && p.preview.is_some())
            && v.shared_phase == Some(cumulus_core::listen_together::IncomingPhase::Applied)
    })
    .await;
    // Privilege metadata is published before the asynchronous native apply.
    // Wait for its PAUSE/seek before injecting a later decoder clock; otherwise
    // that still-running apply can legitimately replace the synthetic 48s.
    position(&client, 44.0).await;
    assert!(audio.state.lock().unwrap().view.paused);
    let before = writes(&api);
    let loads = audio.state.lock().unwrap().loads;
    audio.state.lock().unwrap().view.position = Some(48.0);
    position(&client, 48.0).await;
    send(&im, 20008, json!({"roomId":"test-room","ignoreUserIds":[]}));
    wait(&client, |v| {
        v.progress_ms == Some(48000) && v.pending.is_empty()
    })
    .await;
    let report = last_report(&api);
    assert_eq!(report["progress"], 48000);
    assert_eq!(report["playStatus"], "PAUSE");
    assert_eq!(report["commandType"], "PROGRESS");
    assert_eq!(writes(&api), before + 1);
    assert_eq!(audio.state.lock().unwrap().loads, loads);
    assert!(audio.state.lock().unwrap().view.paused);
    client.close_together().await.unwrap();
}
