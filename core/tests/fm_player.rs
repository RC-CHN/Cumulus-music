#[allow(dead_code)]
#[path = "support/intelligence_input.rs"]
mod ai;
#[allow(dead_code)]
#[path = "support/fm.rs"]
mod domain;
#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    Error,
    account::resources::MutationPhase,
    fm::{player::FmPlayer, *},
    membership::Quality,
    playback::{
        controller::{PlaybackCommand, PlaybackController, PlaybackStatus},
        engine::EndReason,
        source::SourceResolver,
    },
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

struct Native {
    domain: domain::Fixture,
    player: Arc<PlaybackController>,
    device: Arc<engine::Device>,
    host: Arc<FmPlayer>,
    intents: Arc<AtomicU64>,
}

#[tokio::test]
async fn active_ai_requires_exit_and_terminal_fallback_can_enter_fm_and_return_without_a_dangling_owner()
 {
    use cumulus_core::playback::{
        controller::IntelligenceUpdate,
        intelligence::{IntelligenceAction, IntelligenceFailure},
    };
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    let initial = f
        .player
        .command(&token, PlaybackCommand::Snapshot)
        .await
        .unwrap();
    let seeded = f
        .player
        .command(
            &token,
            PlaybackCommand::Intelligence(IntelligenceUpdate {
                expected: initial.fence(),
                action: ai::begin(7, false),
            }),
        )
        .await
        .unwrap();
    let session_id = seeded.intelligence.as_ref().unwrap().id.clone();
    let rejected = f
        .host
        .enter_for(&token, FmSource::Personal, Quality::Standard, f.intent())
        .await;
    assert!(matches!(
        rejected,
        Err(Error::State("leave intelligence before entering FM"))
    ));
    assert_eq!(f.domain.api.count("/api/v1/radio/get"), 0);
    let before = f
        .player
        .command(&token, PlaybackCommand::Snapshot)
        .await
        .unwrap();
    f.player
        .command(
            &token,
            PlaybackCommand::Intelligence(IntelligenceUpdate {
                expected: before.fence(),
                action: IntelligenceAction::Fail {
                    session_id,
                    failure: IntelligenceFailure::RequestFailed,
                },
            }),
        )
        .await
        .unwrap();
    f.domain.api.batch(&[1, 2, 3]);
    f.host
        .enter_for(&token, FmSource::Personal, Quality::Standard, f.intent())
        .await
        .unwrap();
    let during = f.player.snapshot();
    assert!(during.fm);
    assert!(during.intelligence.is_none());
    let left = f.host.leave_for(&token, f.intent()).await.unwrap();
    assert!(!left.fm);
    assert!(left.intelligence.is_none());
    assert_eq!(id(&left), Some("7".into()));
    assert!(
        f.domain
            .store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
            .is_none()
    );
}
impl Native {
    fn new() -> Self {
        let domain = domain::Fixture::new();
        let device = Arc::new(engine::Device::default());
        let player = Arc::new(
            PlaybackController::start(
                domain.session.clone(),
                domain.store.clone(),
                Arc::new(SourceResolver::new(domain.session.clone(), None)),
                engine::audio_engine(device.clone()),
            )
            .unwrap(),
        );
        let intents = Arc::new(AtomicU64::new(0));
        let host = Arc::new(
            FmPlayer::new(domain.session.clone(), player.clone(), intents.clone()).unwrap(),
        );
        Self {
            domain,
            player,
            device,
            host,
            intents,
        }
    }
    fn intent(&self) -> u64 {
        self.intents.fetch_add(1, Ordering::AcqRel) + 1
    }
    async fn enter(&self, ids: &[u64]) {
        self.domain.api.batch(ids);
        self.host
            .enter_for(
                &self.domain.session.token().unwrap(),
                FmSource::Personal,
                Quality::Lossless,
                self.intent(),
            )
            .await
            .unwrap();
    }
    fn end(&self) {
        let id = self.device.state.lock().unwrap().view.entry_id.unwrap();
        engine::event(&self.device, id, EndReason::Eof);
    }
}
fn id(s: &cumulus_core::playback::controller::PlaybackSnapshot) -> Option<String> {
    s.selected.as_ref().map(|e| {
        e.resource["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| e.resource["id"].to_string())
    })
}
async fn feedback(f: &Native, count: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while f.domain.api.count("/api/v1/radio/skip") < count {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn enter_uses_fm_trial_and_leave_restores_complete_normal_queue_position_paused() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, engine::replace(&[90, 91, 92]))
        .await
        .unwrap();
    f.player
        .command(&token, PlaybackCommand::Seek(12.5))
        .await
        .unwrap();
    f.player
        .command(&token, PlaybackCommand::Pause(true))
        .await
        .unwrap();
    let normal = f.player.queue(&token).await.unwrap();
    f.enter(&[1, 2, 3, 4]).await;
    let view = f.player.snapshot();
    assert!(view.fm);
    assert_eq!(id(&view), Some("1".into()));
    assert_eq!(view.actual_quality.as_deref(), Some("lossless"));
    {
        let calls = f.domain.api.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|(p, v)| p == "/api/song/enhance/player/url/v1" && v["trialMode"] == 36)
        );
    }
    let back = f.host.leave_for(&token, f.intent()).await.unwrap();
    assert!(!back.fm);
    assert_eq!(back.state, PlaybackStatus::Paused);
    assert_eq!(back.position, Some(12.5));
    assert_eq!(f.player.queue(&token).await.unwrap(), normal);
    assert!(
        f.domain
            .store
            .get("9", "playback", "fm-backup-queue")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn mode_switches_keep_the_original_backup_and_apply_zone_trial_context() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, engine::replace(&[90, 91]))
        .await
        .unwrap();
    f.player
        .command(&token, PlaybackCommand::Seek(13.0))
        .await
        .unwrap();
    let normal = f.player.queue(&token).await.unwrap();
    f.enter(&[1, 2, 3]).await;
    f.domain.api.batch(&[11, 12, 13]);
    let familiar = FmSource::Familiar {
        source_id: "[\"1\",\"2\"]".into(),
    };
    f.host
        .enter_for(&token, familiar.clone(), Quality::Lossless, f.intent())
        .await
        .unwrap();
    assert_eq!(id(&f.player.snapshot()).as_deref(), Some("11"));
    f.domain.api.batch(&[21, 22, 23]);
    f.domain.api.responses.lock().unwrap().insert("/api/v3/song/detail".into(),serde_json::json!({"code":200,"songs":[domain::song(21),domain::song(22),domain::song(23)]}));
    let zone = FmSource::Zone {
        rule_id: "27001".into(),
        trial_mode: None,
    };
    f.host
        .enter_for(&token, zone.clone(), Quality::Lossless, f.intent())
        .await
        .unwrap();
    assert_eq!(id(&f.player.snapshot()).as_deref(), Some("21"));
    assert_eq!(
        f.domain
            .api
            .calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(p, _)| p == "/api/song/enhance/player/url/v1")
            .unwrap()
            .1["trialMode"],
        121
    );
    assert_eq!(
        f.host
            .service()
            .queue_for(&token, &familiar)
            .unwrap()
            .unwrap()
            .current()
            .unwrap()
            .id,
        "11"
    );
    f.host.leave_for(&token, f.intent()).await.unwrap();
    assert_eq!(f.player.queue(&token).await.unwrap(), normal);
    assert_eq!(f.player.snapshot().position, Some(13.0));
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 0);
}

#[tokio::test]
async fn manual_next_reports_once_and_natural_end_is_owned_by_fm_coordinator() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.enter(&[1, 2, 3, 4]).await;
    f.player
        .command(&token, PlaybackCommand::Seek(12.9))
        .await
        .unwrap();
    assert!(
        f.player
            .command(&token, PlaybackCommand::Next)
            .await
            .is_err()
    );
    let next = f
        .host
        .step_for(&token, FmStep::ManualNext, f.intent())
        .await
        .unwrap();
    assert_eq!(id(&next), Some("2".into()));
    feedback(&f, 1).await;
    {
        let calls = f.domain.api.calls.lock().unwrap();
        let skip = &calls
            .iter()
            .find(|(p, _)| p == "/api/v1/radio/skip")
            .unwrap()
            .1;
        assert_eq!(skip["time"], 12);
        assert_eq!(skip["alg"], "alg-1");
    }
    f.end();
    let next = engine::until(&f.player, |v| {
        id(v).as_deref() == Some("3") && v.state == PlaybackStatus::Playing
    })
    .await;
    assert!(next.fm);
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 1);
    assert_eq!(
        f.host
            .service()
            .queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap()
            .current()
            .unwrap()
            .id,
        "3"
    );
}

#[tokio::test]
async fn changing_repeat_keeps_audio_position_and_natural_single_loop_reloads_at_zero() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.enter(&[1, 2, 3]).await;
    f.player
        .command(&token, PlaybackCommand::Seek(18.0))
        .await
        .unwrap();
    let loads = f.device.state.lock().unwrap().loads;
    let single = f
        .host
        .repeat_for(&token, FmRepeat::One, f.intent())
        .await
        .unwrap();
    assert_eq!(single.position, Some(18.0));
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    f.end();
    engine::until(&f.player, |v| {
        v.state == PlaybackStatus::Playing && v.position == Some(0.0)
    })
    .await;
    assert_eq!(id(&f.player.snapshot()), Some("1".into()));
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 0);
    assert!(f.device.state.lock().unwrap().loads > loads);
}

#[tokio::test]
async fn slow_refill_cannot_override_stop_and_pending_cursor_recovers_without_second_skip() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.enter(&[1, 2, 3]).await;
    f.host
        .step_for(&token, FmStep::ManualNext, f.intent())
        .await
        .unwrap();
    feedback(&f, 1).await;
    tokio::time::sleep(Duration::from_millis(260)).await;
    f.domain.api.batch(&[4, 5, 6]);
    *f.domain.api.hold.lock().unwrap() = Some("/api/v1/radio/get".into());
    let host = f.host.clone();
    let t = token.clone();
    let intent = f.intent();
    let task = tokio::spawn(async move { host.step_for(&t, FmStep::ManualNext, intent).await });
    f.domain.api.started.notified().await;
    f.intent();
    f.player
        .command(&token, PlaybackCommand::Stop)
        .await
        .unwrap();
    f.domain.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(f.player.snapshot().state, PlaybackStatus::Idle);
    assert_eq!(id(&f.player.snapshot()), Some("2".into()));
    assert!(
        f.host
            .pending_for(&token, &f.player.queue(&token).await.unwrap())
            .unwrap()
    );
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 1);
    assert!(
        f.host
            .service()
            .skip_operations_for(&token)
            .unwrap()
            .iter()
            .any(|r| r.phase == SkipPhase::Abandoned)
    );
    let recovered = f.host.resume_for(&token, false, f.intent()).await.unwrap();
    assert_eq!(id(&recovered), Some("3".into()));
    assert_eq!(recovered.state, PlaybackStatus::Playing);
    assert!(
        !f.host
            .pending_for(&token, &f.player.queue(&token).await.unwrap())
            .unwrap()
    );
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 1);
}

#[tokio::test]
async fn late_refill_cannot_replace_a_new_normal_selection_or_leak_to_another_account() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.enter(&[1, 2, 3]).await;
    f.host
        .step_for(&token, FmStep::ManualNext, f.intent())
        .await
        .unwrap();
    feedback(&f, 1).await;
    tokio::time::sleep(Duration::from_millis(260)).await;
    f.domain.api.batch(&[4, 5, 6]);
    *f.domain.api.hold.lock().unwrap() = Some("/api/v1/radio/get".into());
    let host = f.host.clone();
    let t = token.clone();
    let intent = f.intent();
    let task = tokio::spawn(async move { host.step_for(&t, FmStep::Ended, intent).await });
    f.domain.api.started.notified().await;
    f.intent();
    f.player
        .command(&token, engine::replace(&[91]))
        .await
        .unwrap();
    f.domain.api.release.notify_one();
    assert!(task.await.unwrap().is_err());
    assert_eq!(id(&f.player.snapshot()), Some("91".into()));
    // Leaving stale FM responsibility must not replace this newer normal queue.
    f.host.leave_for(&token, f.intent()).await.unwrap();
    assert_eq!(id(&f.player.snapshot()), Some("91".into()));
    f.domain.adopt(10);
    let other = f.domain.session.token().unwrap();
    f.player
        .command(&other, PlaybackCommand::Snapshot)
        .await
        .unwrap();
    assert!(!f.player.snapshot().fm);
    assert!(
        f.host
            .service()
            .queue_for(&other, &FmSource::Personal)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        f.host.resume_for(&token, false, f.intent()).await,
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn uncertain_trash_does_not_change_playback_and_reconciliation_applies_without_skip() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.enter(&[1, 2, 3, 4]).await;
    f.domain
        .api
        .fault("/api/radio/trash/add", domain::Fault::Lost);
    let entry = f.player.snapshot().selected.unwrap().entry_id;
    assert!(f.host.trash_for(&token, &entry, f.intent()).await.is_err());
    assert_eq!(id(&f.player.snapshot()), Some("1".into()));
    let op = f
        .host
        .service()
        .trash_operations_for(&token)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(op.phase, MutationPhase::Uncertain);
    let result = f
        .host
        .apply_confirmed_trash_for(&token, &op.id, f.intent())
        .await
        .unwrap();
    assert_eq!(id(&result), Some("2".into()));
    assert_eq!(f.domain.api.count("/api/radio/trash/add"), 1);
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 0);
}

#[tokio::test]
async fn restart_restores_fm_position_paused_and_keeps_normal_queue_backup() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, engine::replace(&[90, 91]))
        .await
        .unwrap();
    f.enter(&[1, 2, 3]).await;
    f.player
        .command(&token, PlaybackCommand::Seek(14.0))
        .await
        .unwrap();
    f.player
        .command(&token, PlaybackCommand::Pause(true))
        .await
        .unwrap();
    let Native {
        domain,
        player,
        host,
        intents,
        ..
    } = f;
    drop(host);
    drop(player);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let device = Arc::new(engine::Device::default());
    let player = Arc::new(
        PlaybackController::start(
            domain.session.clone(),
            domain.store.clone(),
            Arc::new(SourceResolver::new(domain.session.clone(), None)),
            engine::audio_engine(device),
        )
        .unwrap(),
    );
    let host = FmPlayer::new(domain.session.clone(), player.clone(), intents.clone()).unwrap();
    let restored = host
        .resume_for(&token, true, intents.load(Ordering::Acquire))
        .await
        .unwrap();
    assert!(restored.fm);
    assert_eq!(restored.state, PlaybackStatus::Paused);
    assert_eq!(restored.position, Some(14.0));
    let normal = host
        .leave_for(&token, intents.load(Ordering::Acquire))
        .await
        .unwrap();
    assert_eq!(id(&normal), Some("90".into()));
    assert_eq!(normal.queue_len, 2);
    assert_eq!(normal.state, PlaybackStatus::Paused);
}

#[tokio::test]
async fn startup_fm_policy_handles_saved_and_unapplied_queues_without_repeating_a_skip() {
    use cumulus_core::playback::{controller::StartupRestore, preferences::StartupSettings};
    for changed in [false, true] {
        let f = Native::new();
        let token = f.domain.session.token().unwrap();
        f.enter(&[1, 2, 3]).await;
        f.player
            .command(&token, PlaybackCommand::Seek(14.0))
            .await
            .unwrap();
        f.player
            .command(&token, PlaybackCommand::Stop)
            .await
            .unwrap();
        let Native {
            domain,
            player,
            host,
            intents,
            ..
        } = f;
        let service = host.service().clone();
        drop(host);
        drop(player);
        tokio::time::sleep(Duration::from_millis(50)).await;
        if changed {
            let queue = service
                .queue_for(&token, &FmSource::Personal)
                .unwrap()
                .unwrap();
            service
                .repeat_for(&token, &FmSource::Personal, queue.revision, FmRepeat::One)
                .unwrap();
        }
        let player = Arc::new(
            PlaybackController::start(
                domain.session.clone(),
                domain.store.clone(),
                Arc::new(SourceResolver::new(domain.session.clone(), None)),
                engine::audio_engine(Arc::new(engine::Device::default())),
            )
            .unwrap(),
        );
        let host = FmPlayer::new(domain.session.clone(), player.clone(), intents.clone()).unwrap();
        let view = player
            .command(&token, PlaybackCommand::Snapshot)
            .await
            .unwrap();
        let result = host
            .startup_for(
                &token,
                StartupRestore {
                    expected: view.fence(),
                    queue: None,
                    settings: StartupSettings {
                        auto_play: true,
                        remember_position: !changed,
                    },
                    allow_autoplay: true,
                },
                intents.load(Ordering::Acquire),
            )
            .await
            .unwrap();
        assert!(result.fm);
        assert_eq!(result.state, PlaybackStatus::Playing);
        assert_eq!(result.position, Some(if changed { 0.0 } else { 14.0 }));
        assert_eq!(
            result.mode,
            if changed {
                cumulus_core::playback::PlaybackMode::SingleLoop
            } else {
                cumulus_core::playback::PlaybackMode::Sequential
            }
        );
        assert!(
            !host
                .pending_for(&token, &player.queue(&token).await.unwrap())
                .unwrap()
        );
        assert_eq!(domain.api.count("/api/v1/radio/skip"), 0);
        player.command(&token, PlaybackCommand::Stop).await.unwrap();
    }
}

#[tokio::test]
async fn newly_blocked_current_is_filtered_when_reentering_and_empty_fm_keeps_original_backup() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, engine::replace(&[90]))
        .await
        .unwrap();
    f.enter(&[1, 2, 3]).await;
    f.player
        .command(&token, PlaybackCommand::Stop)
        .await
        .unwrap();
    f.domain
        .api
        .blacklist
        .lock()
        .unwrap()
        .songs
        .insert("1".into());
    f.host
        .enter_for(&token, FmSource::Personal, Quality::Standard, f.intent())
        .await
        .unwrap();
    assert_eq!(id(&f.player.snapshot()), Some("2".into()));
    // Empty-domain installation is a real no-recommendations state.
    f.domain
        .api
        .blacklist
        .lock()
        .unwrap()
        .songs
        .extend(["2".into(), "3".into()]);
    f.player
        .command(&token, PlaybackCommand::Stop)
        .await
        .unwrap();
    f.host
        .enter_for(&token, FmSource::Personal, Quality::Standard, f.intent())
        .await
        .unwrap();
    assert!(f.player.snapshot().selected.is_none());
    assert!(
        f.domain
            .store
            .get("9", "playback", "fm-backup-queue")
            .unwrap()
            .is_some()
    );
    let normal = f.host.leave_for(&token, f.intent()).await.unwrap();
    assert_eq!(id(&normal), Some("90".into()));
}

#[tokio::test]
async fn empty_fm_retains_normal_backup_but_normal_replacement_retires_it_atomically() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, engine::replace(&[90, 91]))
        .await
        .unwrap();
    f.enter(&[]).await;
    let empty = f.player.queue(&token).await.unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(
        f.host.active_source_for(&token, &empty).unwrap(),
        Some(FmSource::Personal)
    );
    let back = f.host.leave_for(&token, f.intent()).await.unwrap();
    assert_eq!(id(&back).as_deref(), Some("90"));
    assert_eq!(back.queue_len, 2);
    f.enter(&[]).await;
    f.player
        .command(&token, engine::replace(&[92]))
        .await
        .unwrap();
    f.player
        .command(&token, engine::replace(&[]))
        .await
        .unwrap();
    let normal = f.player.queue(&token).await.unwrap();
    assert_eq!(f.host.active_source_for(&token, &normal).unwrap(), None);
    assert!(
        f.domain
            .store
            .get("9", "playback", "fm-backup-queue")
            .unwrap()
            .is_none()
    );
    assert!(f.host.leave_for(&token, f.intent()).await.is_err());
}

#[tokio::test]
async fn artist_failure_still_advances_confirmed_song_and_never_sends_skip() {
    let f = Native::new();
    let token = f.domain.session.token().unwrap();
    f.enter(&[1, 2, 3, 4]).await;
    let entry = f.player.snapshot().selected.unwrap().entry_id;
    let choices = f
        .host
        .service()
        .reason_choices_for(&token, &FmSource::Personal, &entry)
        .await
        .unwrap();
    let key = &choices
        .options
        .iter()
        .find(|r| r.artist_id.is_some())
        .unwrap()
        .key;
    f.domain
        .api
        .fault("/api/music-blacklist/add", domain::Fault::Lost);
    assert!(
        f.host
            .feedback_for(&token, &entry, key, f.intent())
            .await
            .is_err()
    );
    assert_eq!(id(&f.player.snapshot()).as_deref(), Some("2"));
    assert_eq!(f.domain.api.count("/api/radio/trash/add"), 1);
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 0);
    let op = f
        .host
        .service()
        .artist_operations_for(&token)
        .unwrap()
        .pop()
        .unwrap();
    f.host
        .service()
        .reconcile_artist_for(&token, &op.id)
        .await
        .unwrap();
    assert_eq!(f.domain.api.count("/api/music-blacklist/add"), 1);
    assert!(
        f.host
            .feedback_for(&token, &entry, key, f.intent())
            .await
            .is_err()
    );
    assert_eq!(f.domain.api.count("/api/radio/trash/add"), 1);
}

#[tokio::test]
async fn checked_repeat_rejects_a_changed_playback_fence_before_mutating_fm() {
    let f = Native::new();
    f.enter(&[1, 2, 3]).await;
    let token = f.domain.session.token().unwrap();
    let old = f.player.snapshot();
    f.player
        .command(&token, PlaybackCommand::Seek(18.0))
        .await
        .unwrap();
    assert!(matches!(
        f.host
            .repeat_checked_for(&token, FmRepeat::One, f.intent(), old.fence())
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        f.player.snapshot().mode,
        cumulus_core::playback::PlaybackMode::Sequential
    );
    assert_eq!(
        f.domain
            .fm
            .queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap()
            .repeat,
        FmRepeat::Infinite
    );
}

#[tokio::test]
async fn repeat_retry_recovers_a_mode_only_commit_failure_without_replaying_a_skip() {
    let f = Native::new();
    f.enter(&[1, 2, 3]).await;
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, PlaybackCommand::Pause(true))
        .await
        .unwrap();
    f.player
        .command(&token, PlaybackCommand::Seek(18.0))
        .await
        .unwrap();
    let before = f.player.snapshot();
    let loads = f.device.state.lock().unwrap().loads;
    let sql = rusqlite::Connection::open(f.domain.store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER deny_mode BEFORE UPDATE ON state WHEN NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic queue mode failure'); END;").unwrap();
    assert!(
        f.host
            .repeat_checked_for(&token, FmRepeat::One, f.intent(), before.fence())
            .await
            .is_err()
    );
    assert_eq!(
        f.player.snapshot().mode,
        cumulus_core::playback::PlaybackMode::Sequential
    );
    assert_eq!(
        f.domain
            .fm
            .queue_for(&token, &FmSource::Personal)
            .unwrap()
            .unwrap()
            .repeat,
        FmRepeat::One
    );
    sql.execute_batch("DROP TRIGGER deny_mode;").unwrap();
    let retry = f
        .host
        .repeat_checked_for(
            &token,
            FmRepeat::One,
            f.intent(),
            f.player.snapshot().fence(),
        )
        .await
        .unwrap();
    assert_eq!(retry.mode, cumulus_core::playback::PlaybackMode::SingleLoop);
    assert_eq!(retry.play_id, before.play_id);
    assert_eq!(retry.position, Some(18.0));
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 0);
}

#[tokio::test]
async fn explicit_resume_repairs_pending_mode_without_restarting_the_current_song() {
    let f = Native::new();
    f.enter(&[1, 2, 3]).await;
    let token = f.domain.session.token().unwrap();
    f.player
        .command(&token, PlaybackCommand::Pause(true))
        .await
        .unwrap();
    f.player
        .command(&token, PlaybackCommand::Seek(18.0))
        .await
        .unwrap();
    let before = f.player.snapshot();
    let loads = f.device.state.lock().unwrap().loads;
    let sql = rusqlite::Connection::open(f.domain.store.directory().join("core.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER deny_resume_mode BEFORE UPDATE ON state WHEN NEW.kind='playback' AND NEW.key='queue' BEGIN SELECT RAISE(ABORT,'synthetic queue mode failure'); END;").unwrap();
    assert!(
        f.host
            .repeat_checked_for(&token, FmRepeat::One, f.intent(), before.fence())
            .await
            .is_err()
    );
    let queue = f.player.queue(&token).await.unwrap();
    assert!(f.host.mode_pending_for(&token, &queue).unwrap());
    sql.execute_batch("DROP TRIGGER deny_resume_mode;").unwrap();
    let resumed = f.host.resume_for(&token, false, f.intent()).await.unwrap();
    assert_eq!(
        resumed.mode,
        cumulus_core::playback::PlaybackMode::SingleLoop
    );
    assert_eq!(resumed.play_id, before.play_id);
    assert_eq!(resumed.position, Some(18.0));
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(f.device.state.lock().unwrap().loads, loads);
    assert!(
        !f.host
            .mode_pending_for(&token, &f.player.queue(&token).await.unwrap())
            .unwrap()
    );
    assert_eq!(f.domain.api.count("/api/v1/radio/skip"), 0);
}
