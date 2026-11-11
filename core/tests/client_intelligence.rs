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

#[tokio::test]
async fn fallback_decoder_is_not_a_public_row_and_displayed_edits_preserve_its_identity() {
    use cumulus_core::{
        client::dto::QueueEdit,
        playback::{
            PlaybackQueue, Step,
            intelligence::{IntelligenceAction, IntelligenceFailure, IntelligencePhase},
        },
    };
    let f = Fixture::persistent();
    let id = act(&f, begin(90, false))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    act(
        &f,
        IntelligenceAction::Fail {
            session_id: id,
            failure: IntelligenceFailure::RequestFailed,
        },
    )
    .await
    .unwrap();
    f.command(Cmd::Stop).await.unwrap();
    let client = MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(f.temp.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(|| Ok(audio_engine(Arc::new(Device::default()))));
    let epoch = client.account().unwrap().epoch;
    client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let playing = client
        .player_action(&epoch, PlayerAction::Seek { seconds: 10.0 })
        .await
        .unwrap();
    assert_eq!(playing.intelligence, Some(IntelligencePhase::Fallback));
    assert_eq!(playing.queue_len, 3);
    let q = client.player_queue(&epoch).await.unwrap();
    assert_eq!(
        q.tracks
            .iter()
            .map(|e| e.track.id.as_str())
            .collect::<Vec<_>>(),
        ["7", "8", "9"]
    );
    assert!(q.current.is_none());
    let mini = client.mini_snapshot().unwrap();
    let page = client.mini_queue(&mini, 1, 1).await.unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.tracks[0].track.id, "8");
    let hidden = playing.entry_id.clone().unwrap();
    for edit in [
        QueueEdit::Select {
            entry_id: hidden.clone(),
        },
        QueueEdit::Remove {
            entry_ids: vec![hidden.clone()],
        },
        QueueEdit::Reorder {
            entry_ids: vec![hidden.clone()],
        },
    ] {
        assert!(matches!(
            client
                .player_action(
                    &epoch,
                    PlayerAction::QueueFor {
                        expected_queue_revision: q.revision.clone(),
                        expected_room_id: None,
                        edit
                    }
                )
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    let reordered = client
        .player_action(
            &epoch,
            PlayerAction::QueueFor {
                expected_queue_revision: q.revision,
                expected_room_id: None,
                edit: QueueEdit::Reorder {
                    entry_ids: q.tracks.iter().rev().map(|e| e.entry_id.clone()).collect(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(reordered.play_id, playing.play_id);
    assert_eq!(reordered.position, playing.position);
    let q = client.player_queue(&epoch).await.unwrap();
    assert_eq!(
        q.tracks
            .iter()
            .map(|e| e.track.id.as_str())
            .collect::<Vec<_>>(),
        ["9", "8", "7"]
    );
    let emptied = client
        .player_action(
            &epoch,
            PlayerAction::QueueFor {
                expected_queue_revision: q.revision,
                expected_room_id: None,
                edit: QueueEdit::Remove {
                    entry_ids: q.tracks.into_iter().map(|e| e.entry_id).collect(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(emptied.queue_len, 0);
    assert_eq!(emptied.play_id, playing.play_id);
    assert_eq!(emptied.position, playing.position);
    assert!(client.player_queue(&epoch).await.unwrap().tracks.is_empty());
    let media = client.native_media_snapshot().unwrap();
    assert!(!media.can_next && !media.can_previous && media.target.is_some());
    let mut native = PlaybackQueue::new(Some(f.store.clone()), "9").unwrap();
    assert_eq!(native.len(), 1);
    assert_eq!(native.visible_len(), 0);
    assert!(native.peek(Step::Next, true).is_none());
    assert!(native.peek(Step::Previous, false).is_none());
    native.set_mode(PlaybackMode::SingleLoop).unwrap();
    assert!(native.peek(Step::Next, true).is_none());
    let dto = serde_json::to_string(&emptied).unwrap();
    assert!(
        !dto.contains("intelligence_transient")
            && !dto.contains("session_id")
            && !dto.contains("backup")
    );
    client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
}

#[tokio::test]
async fn restored_active_intelligence_has_a_distinct_mini_family_and_leaves_through_its_native_target()
 {
    use cumulus_core::{
        client::mini::MiniAction,
        playback::{intelligence::IntelligencePhase, mode::ModeFamily},
    };
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
    client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    let paused = client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.intelligence, Some(IntelligencePhase::Active));
    let shown = client.mini_snapshot().unwrap();
    assert_eq!(shown.view.mode_family, Some(ModeFamily::Intelligence));
    assert!(matches!(
        client
            .mini_action(
                &shown,
                MiniAction::Mode {
                    mode: PlaybackMode::Random
                }
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    client
        .mini_action(
            &shown,
            MiniAction::Mode {
                mode: PlaybackMode::OrderLoop,
            },
        )
        .await
        .unwrap();
    let left = client
        .subscribe_playback(&epoch)
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(left.intelligence.is_none());
    assert_eq!(left.mode, PlaybackMode::OrderLoop);
    assert_eq!(left.queue_len, 3);
    assert_eq!(left.play_id, paused.play_id);
    assert!(matches!(
        client
            .mini_action(
                &shown,
                MiniAction::Mode {
                    mode: PlaybackMode::OrderLoop
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
}

#[tokio::test]
async fn history_play_all_keeps_active_intelligence_on_denial_then_restores_ordinary_mode() {
    use cumulus_core::{
        account::AccountSession,
        client::dto::PlaySelection,
        history::{HistoryKind, HistoryRecord, HistoryService},
        membership::Quality,
        playback::{controller::PlaybackStatus, intelligence::IntelligencePhase},
        protocol::api::{Api, ApiFuture},
    };
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn song(id: &Value) -> Value {
        json!({"id":id,"name":format!("history {id}"),"dt":120000})
    }

    struct HistoryApi {
        audio: Arc<Backend>,
        admit_history: AtomicBool,
    }

    impl Api for HistoryApi {
        fn request<'a>(
            &'a self,
            path: &'a str,
            payload: Value,
            eapi: bool,
        ) -> ApiFuture<'a, Value> {
            match path {
                "/api/v3/song/detail" => Box::pin(async move {
                    let ids: Vec<Value> = serde_json::from_str(payload["c"].as_str().unwrap())?;
                    Ok(
                        json!({"code":200,"songs":ids.iter().map(|row|song(&row["id"])).collect::<Vec<_>>()}),
                    )
                }),
                "/api/song/enhance/privilege" => Box::pin(async move {
                    let ids: Vec<String> = serde_json::from_str(payload["ids"].as_str().unwrap())?;
                    let admit = self.admit_history.load(Ordering::SeqCst);
                    let rows: Vec<_> = ids
                        .iter()
                        .map(|id| {
                            let allowed = admit && matches!(id.as_str(), "41" | "42");
                            json!({"id":id,"pl":if allowed {128000} else {0}})
                        })
                        .collect();
                    Ok(json!({"code":200,"data":rows}))
                }),
                _ => self.audio.request(path, payload, eapi),
            }
        }
    }

    let f = Fixture::persistent();
    f.command(replace(&[7, 8, 9])).await.unwrap();
    f.command(Cmd::Mode(PlaybackMode::Sequential))
        .await
        .unwrap();
    let id = act(&f, begin(8, true))
        .await
        .unwrap()
        .intelligence
        .unwrap()
        .id;
    act(&f, complete(&id, &[90])).await.unwrap();
    f.command(Cmd::Stop).await.unwrap();

    let api = Arc::new(HistoryApi {
        audio: f.api.clone(),
        admit_history: AtomicBool::new(false),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(f.store.clone())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(
        session.clone(),
        ProfileRegistry::open(f.temp.path(), ApiOrigin::Pc).unwrap(),
    )
    .with_audio_factory(|| Ok(audio_engine(Arc::new(Device::default()))));
    let epoch = client.account().unwrap().epoch;
    client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    let paused = client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(paused.intelligence, Some(IntelligencePhase::Active));
    assert_eq!(paused.mode, PlaybackMode::OrderLoop);
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let stored = || {
        f.store
            .get("9", "playback-intelligence-v1", "session")
            .unwrap()
    };
    let active = stored().unwrap();
    assert_eq!(active["last_mode"], "sequential");

    let kind = HistoryKind::Song;
    let history = HistoryService::new(session.clone()).unwrap();
    for (id, at) in [("41", 500), ("42", 400)] {
        history
            .record_for(
                &session.token().unwrap(),
                kind,
                HistoryRecord::local(kind, song(&json!(id)), at).unwrap(),
            )
            .unwrap();
    }
    let revision = client
        .history_page(&epoch, kind, false)
        .await
        .unwrap()
        .value
        .revision;
    let selection = || PlaySelection::HistoryPlayAll {
        history_kind: kind,
        revision: revision.clone(),
        quality: Quality::Standard,
    };
    let ticket = || {
        client
            .prepare_play_selection(&epoch, &paused.queue_revision, paused.play_id.as_deref())
            .unwrap()
    };
    assert!(matches!(
        client
            .commit_prepared_selection(&epoch, &ticket(), selection())
            .await,
        Err(Error::State("history selection has no playable resources"))
    ));
    let still_paused = client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(still_paused.intelligence, paused.intelligence);
    assert_eq!(still_paused.queue_revision, paused.queue_revision);
    assert_eq!(still_paused.play_id, paused.play_id);
    assert_eq!(still_paused.position, paused.position);
    assert_eq!(still_paused.state, paused.state);
    assert_eq!(stored(), Some(active));

    api.admit_history.store(true, Ordering::SeqCst);
    let started = client
        .commit_prepared_selection(&epoch, &ticket(), selection())
        .await
        .unwrap();
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(started.mode, PlaybackMode::Sequential);
    assert_eq!(started.track.unwrap().id, "41");
    assert_ne!(started.play_id, paused.play_id);
    assert!(started.intelligence.is_none());
    assert!(stored().is_none());
    let queue = client.player_queue(&epoch).await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["41", "42"]
    );
    let persisted = f.store.get("9", "playback", "queue").unwrap().unwrap();
    assert_eq!(persisted["mode"], "sequential");
    assert!(persisted["intelligence"].is_null());
}
