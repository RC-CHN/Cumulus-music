#[allow(dead_code)]
#[path = "support/intelligence_player.rs"]
mod support;
use cumulus_core::{
    Error,
    account::profiles::ProfileRegistry,
    client::{
        MusicClient,
        dto::{PlayerAction, TogetherCommand},
    },
    playback::{PlaybackMode, controller::PlaybackCommand as Cmd},
    protocol::http::ApiOrigin,
};
use std::sync::Arc;
use support::*;

#[tokio::test]
async fn a_restored_ai_queue_requires_audio_ownership_to_exit_and_cannot_be_attached_to_a_room() {
    let f = Fixture::persistent();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    let id = act(&f, begin(8, true))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    act(&f, complete(&id, &[90])).await.unwrap();
    f.command(Cmd::Stop).await.unwrap();
    let client = MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(f.temp.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(|| Ok(audio_engine(Arc::new(Device::default()))));
    let epoch = client.account().unwrap().epoch;
    let view = client
        .subscribe_playback(&epoch)
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(matches!(
        client
            .player_action(
                &epoch,
                PlayerAction::ModeFor {
                    mode: PlaybackMode::OrderLoop,
                    expected_queue_revision: view.queue_revision,
                    fm: false
                }
            )
            .await,
        Err(Error::State("resume audio before leaving intelligence"))
    ));
    assert!(matches!(
        client
            .together_command(&epoch, TogetherCommand::Create)
            .await,
        Err(Error::State("leave intelligence before entering together"))
    ));
    let played = client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    let left = client
        .player_action(
            &epoch,
            PlayerAction::ModeFor {
                mode: PlaybackMode::OrderLoop,
                expected_queue_revision: played.queue_revision,
                fm: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(left.play_id, played.play_id);
    assert_eq!(client.player_queue(&epoch).await.unwrap().tracks.len(), 3);
    assert!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
}
