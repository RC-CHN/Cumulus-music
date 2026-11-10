use super::*;
use cumulus_core::membership::Quality;
use cumulus_core::playback::controller::PlaybackStatus;

fn songs(ids: &[&str], index: usize) -> PlaySelection {
    PlaySelection::Songs {
        ids: ids.iter().map(|id| (*id).to_owned()).collect(),
        index,
        quality: Quality::Standard,
    }
}

fn calls(api: &Backend, suffix: &str) -> usize {
    api.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.ends_with(suffix))
        .count()
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
async fn filters_known_catalog_denials_in_order_and_keeps_the_rejected_explicit_anchors_decoder() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| {
        v.attached && v.target_id.as_deref() == Some("91")
    })
    .await;
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&client, |v| !v.playing).await;
    let media_reads = calls(&api, "/player/url/v1");
    let writes = calls(&api, "/sync/list/command/report");
    let value = client
        .play_selection("1", songs(&["710", "712", "714", "713", "715", "711"], 0))
        .await
        .unwrap();
    assert_eq!(value.state, PlaybackStatus::Paused);
    assert_eq!(value.play_id, before.play_id);
    assert_eq!(value.position, before.position);
    assert_eq!(value.track.as_ref().unwrap().id, "91");
    assert_ne!(value.queue_revision, before.queue_revision);
    assert_eq!(queue_ids(&client).await, ["714", "715"]);
    let after = wait(&client, |v| v.external_selection.is_some()).await;
    let notice = after.external_selection.unwrap();
    assert!(!notice.blocked);
    assert_eq!(notice.skipped, 4);
    assert_eq!(calls(&api, "/player/url/v1"), media_reads);
    assert_eq!(calls(&api, "/sync/list/command/report"), writes);
    assert_eq!(calls(&api, "/end/v2"), 0);

    let playable = client
        .play_selection("1", songs(&["710", "714", "711", "715"], 3))
        .await
        .unwrap();
    assert_eq!(playable.track.as_ref().unwrap().id, "715");
    assert_eq!(queue_ids(&client).await, ["714", "715"]);
    wait(&client, |v| {
        v.target_id.as_deref() == Some("715") && v.tracks.len() == 2
    })
    .await;
}

#[tokio::test]
async fn all_unsupported_requires_the_exact_room_and_an_explicit_confirmation_and_never_replays() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = world(root.path(), api.clone());
    play(&client).await;
    client
        .together_command("1", TogetherCommand::Create)
        .await
        .unwrap();
    wait(&client, |v| {
        v.attached && v.target_id.as_deref() == Some("91")
    })
    .await;
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    wait(&client, |v| !v.playing).await;

    for confirm in [false, true] {
        assert!(matches!(
            client
                .play_selection("1", songs(&["710", "712", "713", "711"], 0))
                .await,
            Err(Error::TogetherSelectionUnsupported)
        ));
        let notice = wait(&client, |v| v.external_selection.is_some())
            .await
            .external_selection
            .unwrap();
        assert!(notice.blocked);
        assert_eq!(notice.skipped, 4);
        let current = client.player_snapshot("1").await.unwrap();
        assert_eq!(current.play_id, before.play_id);
        assert_eq!(current.position, before.position);
        assert_eq!(current.queue_revision, before.queue_revision);
        assert_eq!(calls(&api, "/end/v2"), 0);

        assert!(matches!(
            client
                .together_command(
                    "1",
                    TogetherCommand::ResolveExternalSelection {
                        room_id: "other-room".into(),
                        notice_id: notice.id.clone(),
                        confirm: true,
                    }
                )
                .await,
            Err(Error::StaleResponse)
        ));
        let command = || TogetherCommand::ResolveExternalSelection {
            room_id: notice.room_id.clone(),
            notice_id: notice.id.clone(),
            confirm,
        };
        let view = client.together_command("1", command()).await.unwrap().value;
        assert_eq!(view.room.is_none(), confirm);
        assert!(view.external_selection.is_none());
        assert!(matches!(
            client.together_command("1", command()).await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(calls(&api, "/end/v2"), 1);
    assert_eq!(queue_ids(&client).await, ["91", "92", "93"]);
    let current = client.player_snapshot("1").await.unwrap();
    assert_eq!(current.play_id, before.play_id);
    assert_eq!(current.track.as_ref().unwrap().id, "91");
}
