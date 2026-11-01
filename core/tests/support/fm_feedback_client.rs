use super::*;

#[tokio::test]
async fn an_old_feedback_dialog_cannot_cancel_an_accepted_fm_next() {
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3]);
    let opened = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let old_entry = opened.value.current.unwrap();
    client.player_action("1", PlayerAction::Next).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(260)).await;
    api.batch(&[5, 6, 7]);
    *api.hold.lock().unwrap() = Some("/api/v1/radio/get".into());
    let next_client = client.clone();
    let next =
        tokio::spawn(async move { next_client.player_action("1", PlayerAction::Next).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    let mut rejected = Vec::new();
    for command in [
        FmCommand::Trash {
            entry_id: old_entry.clone(),
        },
        FmCommand::Feedback {
            entry_id: old_entry,
            reason_key: "obsolete-choice".into(),
        },
    ] {
        rejected.push(client.fm_command("1", FmSource::Personal, command).await);
    }
    api.release.notify_one();
    let changed = next
        .await
        .unwrap()
        .expect("an obsolete feedback window must not cancel the accepted Next");
    assert_eq!(changed.track.unwrap().id, "3");
    assert!(
        rejected
            .into_iter()
            .all(|result| matches!(result, Err(Error::QueueChanged)))
    );
    assert_eq!(api.count("/api/radio/trash/add"), 0);
    assert_eq!(api.count("/api/v1/radio/get"), 2);
}
