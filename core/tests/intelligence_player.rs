#[allow(dead_code)]
#[path = "support/intelligence_player.rs"]
mod support;
use cumulus_core::playback::{
    PlaybackMode,
    controller::{PlaybackCommand as Cmd, PlaybackStatus},
    intelligence::{IntelligenceAction, IntelligenceFailure, IntelligencePhase},
};
use serde_json::json;
use support::*;

#[tokio::test]
async fn seed_is_played_first_and_recommendations_deduplicate_without_reloading_or_losing_pause() {
    let f = Fixture::new();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    f.command(Cmd::Mode(PlaybackMode::Random)).await.unwrap();
    let backup = f.player.queue(&f.token()).await.unwrap();
    let seeded = act(&f, begin(8, true)).await.unwrap();
    assert_eq!(
        seeded.intelligence.as_ref().unwrap().phase,
        IntelligencePhase::Seeded
    );
    assert_eq!(seeded.queue_len, 1);
    assert_eq!(song(&seeded), Some(8));
    let id = seeded.intelligence.unwrap().id;
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Seek(18.0)).await.unwrap();
    let before = f.command(Cmd::Snapshot).await.unwrap();
    let loads = f.device.state.lock().unwrap().loads;
    let active = act(&f, complete(&id, &[90, 8, 91, 90])).await.unwrap();
    let queue = f.player.queue(&f.token()).await.unwrap();
    assert_eq!(ids(&queue), vec![json!(90), json!(8), json!(91)]);
    assert_eq!(
        active.intelligence.unwrap().phase,
        IntelligencePhase::Active
    );
    assert_eq!(active.play_id, before.play_id);
    assert_eq!(active.position, Some(18.0));
    assert_eq!(active.state, PlaybackStatus::Paused);
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    let restored = act(
        &f,
        IntelligenceAction::Leave {
            session_id: id,
            mode: PlaybackMode::Random,
            retain_missing_current: false,
        },
    )
    .await
    .unwrap();
    assert!(restored.intelligence.is_none());
    assert_eq!(restored.play_id, before.play_id);
    assert_eq!(restored.position, Some(18.0));
    assert_eq!(restored.state, PlaybackStatus::Paused);
    assert_eq!(restored.mode, PlaybackMode::Random);
    assert_eq!(
        restored.selected.as_ref().unwrap().source,
        backup.items[1].source
    );
    let mut expected_order = backup.order;
    for entry in &mut expected_order {
        if *entry == backup.items[1].entry_id {
            *entry = restored.selected.as_ref().unwrap().entry_id.clone();
        }
    }
    assert_eq!(
        f.player.queue(&f.token()).await.unwrap().order,
        expected_order
    );
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    assert!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn manual_queue_changes_before_the_reply_insert_recommendations_after_the_new_current() {
    let f = Fixture::new();
    let seeded = act(&f, begin(7, false)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    f.command(Cmd::Append {
        resources: vec![json!({"id":50}), json!({"id":51}), json!({"id":52})],
        source: json!({"kind":"test"}),
    })
    .await
    .unwrap();
    let queue = f.player.queue(&f.token()).await.unwrap();
    f.command(Cmd::Select(queue.items[2].entry_id.clone()))
        .await
        .unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    let current = f.command(Cmd::Seek(13.0)).await.unwrap();
    let loads = f.device.state.lock().unwrap().loads;
    let applied = act(&f, complete(&id, &[50, 60, 50, 61])).await.unwrap();
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![
            json!(7),
            json!(51),
            json!(50),
            json!(60),
            json!(61),
            json!(52)
        ]
    );
    assert_eq!(applied.play_id, current.play_id);
    assert_eq!(applied.position, Some(13.0));
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
}

#[tokio::test]
async fn failed_read_falls_back_to_liked_loop_and_an_unlisted_audible_seed_is_removed_after_advancing()
 {
    let f = Fixture::new();
    let seeded = act(&f, begin(77, false)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    f.command(Cmd::Pause(true)).await.unwrap();
    let before = f.command(Cmd::Seek(18.0)).await.unwrap();
    let fallback = act(
        &f,
        IntelligenceAction::Fail {
            session_id: id.clone(),
            failure: IntelligenceFailure::RequestFailed,
        },
    )
    .await
    .unwrap();
    assert_eq!(fallback.play_id, before.play_id);
    assert_eq!(fallback.position, Some(18.0));
    assert_eq!(fallback.mode, PlaybackMode::OrderLoop);
    assert_eq!(
        fallback.intelligence.unwrap().phase,
        IntelligencePhase::Fallback
    );
    let queue = f.player.queue(&f.token()).await.unwrap();
    assert_eq!(ids(&queue), vec![json!(77), json!(7), json!(8), json!(9)]);
    assert_eq!(queue.items[0].source["intelligence_transient"], true);
    let next = f.command(Cmd::Next).await.unwrap();
    assert_eq!(song(&next), Some(7));
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![json!(7), json!(8), json!(9)]
    );
    let ordinary = f
        .command(Cmd::Mode(PlaybackMode::Sequential))
        .await
        .unwrap();
    assert!(ordinary.intelligence.is_none());
    assert!(
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
    assert!(act(&f, complete(&id, &[99])).await.is_err());
}

#[tokio::test]
async fn exiting_with_an_unlisted_current_song_plays_the_first_original_song_in_the_default_branch()
{
    let f = Fixture::new();
    f.command(replace(&[11, 12, 13])).await.unwrap();
    let seeded = act(&f, begin(7, true)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    act(&f, complete(&id, &[90, 91])).await.unwrap();
    f.command(Cmd::Next).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Seek(16.0)).await.unwrap();
    let left = act(
        &f,
        IntelligenceAction::Leave {
            session_id: id,
            mode: PlaybackMode::Sequential,
            retain_missing_current: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(song(&left), Some(11));
    assert_eq!(left.position, Some(0.0));
    assert_eq!(left.state, PlaybackStatus::Playing);
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![json!(11), json!(12), json!(13)]
    );
}

#[tokio::test]
async fn experimental_exit_inserts_after_the_nearest_common_predecessor_and_drops_it_after_unliked_reentry()
 {
    let f = Fixture::new();
    f.command(replace(&[11, 12, 13])).await.unwrap();
    let seeded = act(&f, begin(12, true)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    act(&f, complete(&id, &[90, 13, 91])).await.unwrap();
    let queue = f.player.queue(&f.token()).await.unwrap();
    let selected = queue
        .items
        .iter()
        .find(|e| e.resource["id"] == 91)
        .unwrap()
        .entry_id
        .clone();
    f.command(Cmd::Select(selected)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    let before = f.command(Cmd::Seek(18.0)).await.unwrap();
    let left = act(
        &f,
        IntelligenceAction::Leave {
            session_id: id,
            mode: PlaybackMode::OrderLoop,
            retain_missing_current: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(left.play_id, before.play_id);
    assert_eq!(left.position, Some(18.0));
    assert_eq!(left.state, PlaybackStatus::Paused);
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![json!(11), json!(12), json!(13), json!(91)]
    );
    let next = act(&f, begin(7, true)).await.unwrap();
    let id = next.intelligence.unwrap().id;
    act(&f, complete(&id, &[])).await.unwrap();
    act(
        &f,
        IntelligenceAction::Leave {
            session_id: id,
            mode: PlaybackMode::OrderLoop,
            retain_missing_current: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![json!(11), json!(12), json!(13)]
    );
}

#[tokio::test]
async fn native_mode_order_loop_exits_even_though_intelligence_uses_that_same_traversal_underneath()
{
    let f = Fixture::new();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    let seeded = act(&f, begin(7, true)).await.unwrap();
    let id = seeded.intelligence.unwrap().id;
    act(&f, complete(&id, &[90])).await.unwrap();
    let before = f.command(Cmd::Pause(true)).await.unwrap();
    let left = f.command(Cmd::Mode(PlaybackMode::OrderLoop)).await.unwrap();
    assert!(left.intelligence.is_none());
    assert_eq!(left.play_id, before.play_id);
    assert_eq!(
        ids(&f.player.queue(&f.token()).await.unwrap()),
        vec![json!(7), json!(8), json!(9)]
    );
}
