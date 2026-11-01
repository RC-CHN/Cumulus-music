use super::*;

async fn start(client: &MusicClient, api: &domain::Backend) {
    api.batch(&[1, 2, 3, 4, 5, 6]);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action("1", PlayerAction::Seek { seconds: 10.0 })
        .await
        .unwrap();
}

#[tokio::test]
async fn feedback_view_is_local_and_the_actual_offered_artist_uses_the_shared_fm_flow() {
    let (_dir, api, _session, client) = fixture();
    start(&client, &api).await;
    let shown = client.mini_snapshot().unwrap();
    let calls = api.calls.lock().unwrap().len();
    let view = client.mini_fm_feedback_view(&shown, false).await.unwrap();
    assert!(!view.pending);
    assert_eq!(
        Some(view.entry_id.as_str()),
        shown.view.selection.audio_entry_id.as_deref()
    );
    assert_eq!(api.calls.lock().unwrap().len(), calls);
    let choices = client.mini_fm_feedback_reasons(&shown).await.unwrap();
    let key = choices
        .options
        .iter()
        .find(|r| r.artist_id.is_some())
        .unwrap()
        .key
        .clone();
    client.mini_fm_feedback(&shown, Some(key)).await.unwrap();
    assert_eq!(api.count("/api/personalized/dislike/reason"), 1);
    assert_eq!(api.count("/api/radio/trash/add"), 1);
    assert_eq!(api.count("/api/music-blacklist/add"), 1);
    assert!(api.trash.lock().unwrap().contains("1"));
    let state = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(state.fm);
    assert_eq!(state.track.unwrap().id, "2");
}

#[tokio::test]
async fn delayed_reasons_and_old_menu_submission_cannot_touch_the_next_track() {
    let (_dir, api, _session, client) = fixture();
    start(&client, &api).await;
    let old = client.mini_snapshot().unwrap();
    *api.hold.lock().unwrap() = Some("/api/personalized/dislike/reason".into());
    let reader = client.clone();
    let frozen = old.clone();
    let reasons = tokio::spawn(async move { reader.mini_fm_feedback_reasons(&frozen).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    client.player_action("1", PlayerAction::Next).await.unwrap();
    api.release.notify_one();
    assert!(matches!(
        reasons.await.unwrap(),
        Err(Error::StaleResponse | Error::QueueChanged)
    ));
    assert!(client.mini_fm_feedback(&old, None).await.is_err());
    assert_eq!(api.count("/api/radio/trash/add"), 0);
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .track
            .unwrap()
            .id,
        "2"
    );
}

#[tokio::test]
async fn uncertain_write_is_not_reposted_by_a_fresh_mini_ticket_or_readonly_refresh() {
    let (_dir, api, _session, client) = fixture();
    start(&client, &api).await;
    let shown = client.mini_snapshot().unwrap();
    api.fault("/api/radio/trash/add", domain::Fault::Lost);
    assert!(client.mini_fm_feedback(&shown, None).await.is_err());
    let fresh = client.mini_snapshot().unwrap();
    assert!(
        client
            .mini_fm_feedback_view(&fresh, false)
            .await
            .unwrap()
            .pending
    );
    assert!(matches!(
        client.mini_fm_feedback(&fresh, None).await,
        Err(Error::MutationUncertain)
    ));
    assert!(
        client
            .mini_fm_feedback_view(&fresh, true)
            .await
            .unwrap()
            .pending
    );
    assert_eq!(api.count("/api/radio/trash/add"), 1);
    assert_eq!(api.count("/api/v1/radio/get"), 1);
    let view = client
        .fm_view("1", FmSource::Personal, false)
        .await
        .unwrap()
        .value;
    let id = view
        .operations
        .iter()
        .find(|op| op.kind == "trash")
        .unwrap()
        .id
        .clone();
    client
        .fm_command("1", FmSource::Personal, FmCommand::Reconcile { id })
        .await
        .unwrap();
    assert_eq!(api.count("/api/radio/trash/add"), 1);
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .track
            .unwrap()
            .id,
        "2"
    );
}

#[tokio::test]
async fn known_refusal_can_only_be_submitted_again_by_a_new_explicit_decision() {
    let (_dir, api, _session, client) = fixture();
    start(&client, &api).await;
    api.fault("/api/radio/trash/add", domain::Fault::Reject(403));
    assert!(
        client
            .mini_fm_feedback(&client.mini_snapshot().unwrap(), None)
            .await
            .is_err()
    );
    let fresh = client.mini_snapshot().unwrap();
    assert!(
        !client
            .mini_fm_feedback_view(&fresh, true)
            .await
            .unwrap()
            .pending
    );
    assert_eq!(api.count("/api/radio/trash/add"), 1);
    client.mini_fm_feedback(&fresh, None).await.unwrap();
    assert_eq!(api.count("/api/radio/trash/add"), 2);
}

#[tokio::test]
async fn partial_artist_failure_blocks_feedback_for_the_new_song_until_main_reconciliation() {
    let (_dir, api, _session, client) = fixture();
    start(&client, &api).await;
    let shown = client.mini_snapshot().unwrap();
    let choices = client.mini_fm_feedback_reasons(&shown).await.unwrap();
    let key = choices
        .options
        .iter()
        .find(|r| r.artist_id.is_some())
        .unwrap()
        .key
        .clone();
    api.fault("/api/music-blacklist/add", domain::Fault::Reject(403));
    assert!(client.mini_fm_feedback(&shown, Some(key)).await.is_err());
    let next = client.mini_snapshot().unwrap();
    assert!(
        client
            .mini_fm_feedback_view(&next, false)
            .await
            .unwrap()
            .pending
    );
    assert!(matches!(
        client.mini_fm_feedback(&next, None).await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(api.count("/api/radio/trash/add"), 1);
    assert_eq!(api.count("/api/music-blacklist/add"), 1);
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .track
            .unwrap()
            .id,
        "2"
    );
}

#[tokio::test]
async fn account_source_and_empty_fm_boundaries_are_checked_before_feedback_requests() {
    let (_dir, api, session, client) = fixture();
    let empty = client.mini_snapshot().unwrap();
    assert!(client.mini_fm_feedback_view(&empty, false).await.is_err());
    assert_eq!(api.count("/api/v1/radio/get"), 0);
    start(&client, &api).await;
    let shown = client.mini_snapshot().unwrap();
    client
        .fm_command(
            "1",
            FmSource::Zone {
                rule_id: "27001".into(),
                trial_mode: None,
            },
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert!(client.mini_fm_feedback_reasons(&shown).await.is_err());
    assert!(client.mini_fm_feedback(&shown, None).await.is_err());
    let shown = client.mini_snapshot().unwrap();
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(client.mini_fm_feedback_reasons(&shown).await.is_err());
    assert!(client.mini_fm_feedback(&shown, None).await.is_err());
    assert_eq!(api.count("/api/personalized/dislike/reason"), 0);
    assert_eq!(api.count("/api/radio/trash/add"), 0);
}

#[tokio::test]
async fn a_feedback_rejected_while_next_is_refilling_cannot_cancel_that_accepted_next() {
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3]);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client.player_action("1", PlayerAction::Next).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(260)).await;
    api.batch(&[5, 6, 7]);
    *api.hold.lock().unwrap() = Some("/api/v1/radio/get".into());
    let transport = client.clone();
    let next = tokio::spawn(async move { transport.player_action("1", PlayerAction::Next).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    let shown = client.mini_snapshot().unwrap();
    let refused = client.mini_fm_feedback(&shown, None).await;
    api.release.notify_one();
    assert!(matches!(
        refused,
        Err(Error::MutationBusy | Error::StaleResponse)
    ));
    let result = next
        .await
        .unwrap()
        .expect("a rejected mini feedback must preserve accepted Next");
    assert_eq!(result.track.unwrap().id, "3");
    assert_eq!(api.count("/api/radio/trash/add"), 0);
}

#[tokio::test]
async fn a_refused_next_cannot_cancel_feedback_that_has_already_reached_the_service() {
    let (_dir, api, _session, client) = fixture();
    start(&client, &api).await;
    let shown = client.mini_snapshot().unwrap();
    *api.hold.lock().unwrap() = Some("/api/radio/trash/add".into());
    let writer = client.clone();
    let submitted = tokio::spawn(async move { writer.mini_fm_feedback(&shown, None).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    let next = client.player_action("1", PlayerAction::Next).await;
    api.release.notify_one();
    assert!(matches!(next, Err(Error::MutationBusy)));
    submitted
        .await
        .unwrap()
        .expect("refused Next must preserve the accepted feedback");
    assert_eq!(
        client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap()
            .track
            .unwrap()
            .id,
        "2"
    );
    assert_eq!(api.count("/api/radio/trash/add"), 1);
}
