#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::playback::controller::{
    PlaybackCommand as Cmd, PlaybackSpeedScope as Scope, PlaybackStatus,
};
use serde_json::json;
use support::Fixture;

#[tokio::test]
async fn legacy_rate_splits_on_edit_and_the_decoder_uses_the_selected_media_rate() {
    let f = Fixture::new();
    let old = json!({"volume":80.0,"speed":1.25});
    f.store.put("9", "playback", "preferences", &old).unwrap();
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token())
        .unwrap();
    assert_eq!(f.command(Cmd::Snapshot).await.unwrap().speed, 1.0);
    f.session
        .adopt(f.api.clone(), json!({"userId":9}), &f.token())
        .unwrap();
    let song = f
        .command(Cmd::Replace {
            resources: vec![
                json!({"id":1,"dt":120000}),
                json!({"id":2,"voice_id":22,"dt":120000}),
            ],
            index: 0,
            source: json!({"kind":"test"}),
        })
        .await
        .unwrap();
    assert_eq!(song.speed_scope, Scope::Song);
    assert_eq!(song.speed, 1.25);
    assert_eq!(
        f.store.get("9", "playback", "preferences").unwrap(),
        Some(old)
    );
    let set_song = f.command(Cmd::Speed(0.75)).await.unwrap();
    assert_eq!(set_song.play_id, song.play_id);
    assert_eq!(set_song.fence(), song.fence());
    assert_eq!(f.device.state.lock().unwrap().view.speed, Some(0.75));

    let voice = f.command(Cmd::Next).await.unwrap();
    assert_eq!(voice.speed_scope, Scope::Voice);
    assert_eq!(voice.speed, 1.25);
    assert_eq!(f.device.state.lock().unwrap().view.speed, Some(1.25));
    let voice = f.command(Cmd::Speed(1.5)).await.unwrap();
    assert_eq!(voice.speed, 1.5);
    assert_eq!(f.device.state.lock().unwrap().view.speed, Some(1.5));

    let song = f.command(Cmd::Previous).await.unwrap();
    assert_eq!(song.speed_scope, Scope::Song);
    assert_eq!(song.speed, 0.75);
    assert_eq!(f.device.state.lock().unwrap().view.speed, Some(0.75));
    // A speed menu initiated on the voice selection can arrive after a skip.
    // It must save the displayed media's preference without changing audio.
    let unchanged = f
        .command(Cmd::SpeedFor {
            scope: Scope::Voice,
            value: 2.0,
        })
        .await
        .unwrap();
    assert_eq!(unchanged.fence(), song.fence());
    assert_eq!(unchanged.speed, 0.75);
    assert_eq!(f.device.state.lock().unwrap().view.speed, Some(0.75));

    let voice = f.command(Cmd::Next).await.unwrap();
    assert_eq!(voice.speed, 2.0);
    let normal_song = f
        .command(Cmd::SpeedFor {
            scope: Scope::Song,
            value: 1.0,
        })
        .await
        .unwrap();
    assert_eq!(normal_song.fence(), voice.fence());
    assert_eq!(normal_song.speed, 2.0);
    assert_eq!(f.device.state.lock().unwrap().view.speed, Some(2.0));
    let reopened = f.another_player();
    let restored = reopened.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(restored.state, PlaybackStatus::Paused);
    assert_eq!(restored.speed_scope, Scope::Voice);
    assert_eq!(restored.speed, 2.0);
    let song = reopened.command(&f.token(), Cmd::Previous).await.unwrap();
    assert_eq!(song.speed, 1.0);
}
