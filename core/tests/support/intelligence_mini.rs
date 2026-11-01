use super::*;
use cumulus_core::{
    client::mini::{MiniAction, MiniIntelligenceCommand, MiniSnapshot},
    playback::{PlaybackMode, mode::ModeFamily},
};

async fn liked(f: &Fixture) -> MiniSnapshot {
    f.client.playlists().await.unwrap();
    f.client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: "31".into(),
                start_id: Some("7".into()),
                revision: None,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    f.client.mini_snapshot().unwrap()
}

#[tokio::test]
async fn mini_cache_is_only_a_local_hint_and_start_rechecks_the_actual_owner() {
    let f = Fixture::new();
    let shown = liked(&f).await;
    assert!(shown.view.can_intelligence);
    let calls = f.api.calls.lock().unwrap().len();
    for _ in 0..3 {
        assert!(f.client.mini_snapshot().unwrap().view.can_intelligence);
    }
    assert_eq!(f.api.calls.lock().unwrap().len(), calls);
    f.api.foreign.store(true, Ordering::SeqCst);
    assert!(
        f.client
            .mini_intelligence(
                &shown,
                MiniIntelligenceCommand::Start {
                    operation_id: operation()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(f.revision().await, shown.view.queue_revision.unwrap());
    assert!(f.requests().is_empty());
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert!(
        f.client
            .mini_snapshot()
            .unwrap()
            .view
            .intelligence_read
            .is_none()
    );
}

#[tokio::test]
async fn mini_enters_the_real_current_coordinator_and_returns_an_exact_public_receipt() {
    let f = Fixture::new();
    let old = liked(&f).await;
    let operation_id = operation();
    let result = f
        .client
        .mini_intelligence(
            &old,
            MiniIntelligenceCommand::Start {
                operation_id: operation_id.clone(),
            },
        )
        .await
        .unwrap();
    let shown = f.client.mini_snapshot().unwrap();
    assert_eq!(result.intelligence, Some(IntelligencePhase::Active));
    assert_eq!(
        result.intelligence_operation.as_deref(),
        Some(operation_id.as_str())
    );
    assert_eq!(
        shown.view.queue_revision.as_ref(),
        Some(&result.queue_revision)
    );
    assert_eq!(shown.view.mode_family, Some(ModeFamily::Intelligence));
    assert_eq!(f.requests()[0]["type"], "fromPlayOne");
    let private = f.saved().intelligence_session().unwrap().context.id.clone();
    let public = serde_json::to_string(&result).unwrap();
    assert!(
        !public.contains(&private) && !public.contains("SECRET") && !public.contains("request")
    );
    assert!(matches!(
        f.client.mini_mode(&old, PlaybackMode::SingleLoop).await,
        Err(Error::StaleResponse)
    ));
    let before = f.requests().len();
    assert!(
        f.client
            .mini_intelligence(
                &old,
                MiniIntelligenceCommand::Start {
                    operation_id: operation()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(f.requests().len(), before);
}

#[tokio::test]
async fn mini_recommendation_wait_allows_transport_then_exit_without_releasing_http() {
    let f = Fixture::new();
    let shown = liked(&f).await;
    let operation_id = operation();
    f.api.hold_recommend.store(true, Ordering::SeqCst);
    let c = f.client.clone();
    let id = operation_id.clone();
    let task = tokio::spawn(async move {
        c.mini_intelligence(&shown, MiniIntelligenceCommand::Start { operation_id: id })
            .await
    });
    wait(&f.api.recommend).await;
    let held = f.client.mini_snapshot().unwrap();
    assert_eq!(
        held.view.intelligence_read.as_ref().unwrap().operation_id,
        operation_id
    );
    assert!(!held.view.intelligence_read.as_ref().unwrap().preparing);
    f.client
        .mini_action(&held, MiniAction::Toggle)
        .await
        .unwrap();
    f.client
        .mini_action(
            &f.client.mini_snapshot().unwrap(),
            MiniAction::Seek { seconds: 10.0 },
        )
        .await
        .unwrap();
    let paused = f.client.mini_snapshot().unwrap();
    let left = f
        .client
        .mini_mode(&paused, PlaybackMode::OrderLoop)
        .await
        .unwrap();
    assert_eq!(left.play_id, paused.view.selection.audio_play_id);
    assert!(left.intelligence.is_none());
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Cancelled | Error::StaleResponse)
    ));
    let current = f.client.mini_snapshot().unwrap();
    assert!(!current.view.playing && current.view.position == Some(10.0));
    assert!(current.view.intelligence_read.is_none());
    assert_eq!(f.device.state.lock().unwrap().loads, 2);
}

#[tokio::test]
async fn mini_continue_uses_its_new_uuid_but_keeps_the_current_decoder() {
    let f = Fixture::new();
    let shown = liked(&f).await;
    f.api.hold_recommend.store(true, Ordering::SeqCst);
    let id = operation();
    let copy = id.clone();
    let c = f.client.clone();
    let task = tokio::spawn(async move {
        c.mini_intelligence(
            &shown,
            MiniIntelligenceCommand::Start { operation_id: copy },
        )
        .await
    });
    wait(&f.api.recommend).await;
    f.client.player_intelligence_cancel("1", &id).unwrap();
    assert!(task.await.unwrap().is_err());
    let held = f.client.mini_snapshot().unwrap();
    f.client
        .mini_action(&held, MiniAction::Toggle)
        .await
        .unwrap();
    let before = f.client.mini_snapshot().unwrap();
    f.api.hold_recommend.store(false, Ordering::SeqCst);
    let result = f
        .client
        .mini_intelligence(
            &before,
            MiniIntelligenceCommand::Continue {
                operation_id: operation(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.play_id, before.view.selection.audio_play_id);
    assert_eq!(result.intelligence, Some(IntelligencePhase::Active));
    assert_eq!(result.intelligence_operation.as_deref(), Some(id.as_str()));
    assert!(!f.client.mini_snapshot().unwrap().view.playing);
    assert_eq!(f.requests().len(), 2);
    assert_eq!(f.device.state.lock().unwrap().loads, 2);
}
