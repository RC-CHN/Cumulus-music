#[allow(dead_code)]
#[path = "support/fm.rs"]
mod domain;
#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    membership::Quality,
    playback::PlaybackMode,
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::sync::Arc;
fn fixture() -> (
    tempfile::TempDir,
    Arc<domain::Backend>,
    Arc<AccountSession>,
    Arc<MusicClient>,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(domain::Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    (dir, api, session, Arc::new(client))
}

#[tokio::test]
async fn discovery_projection_strips_private_urls_and_transport_requires_the_displayed_source() {
    let (_dir, api, _session, client) = fixture();
    api.responses.lock().unwrap().insert("/api/pc/daily/rcmd/block".into(),json!({"code":200,"data":{"items":[
        {"resourceType":"similarSong","resourceId":"[\"1\",\"2\"]","coverText":"similar","coverUrl":"https://p1.music.126.net/cover.png?secret=PRIVATE","algorithm":"PRIVATE"},
        {"resourceType":"fm","coverUrl":"https://evil.test/PRIVATE"}
    ]}}));
    api.responses.lock().unwrap().insert(
        "/api/secondary/tab/list".into(),
        json!({"code":200,"data":{"hints":[]}}),
    );
    let page = client
        .fm_discovery("1", FmDiscoveryQuery::Home, true)
        .await
        .unwrap();
    assert_eq!(page.value.page.cards.len(), 2);
    assert!(page.value.page.cards[1].cover_url.is_none());
    assert!(!serde_json::to_string(&page).unwrap().contains("PRIVATE"));
    api.batch(&[1, 2, 3]);
    let familiar = FmSource::Familiar {
        source_id: "[\"1\",\"2\"]".into(),
    };
    let playing = client
        .fm_command(
            "1",
            familiar.clone(),
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(playing.value.active_source, Some(familiar.clone()));
    for command in [
        FmCommand::Leave,
        FmCommand::Resume,
        FmCommand::Trash {
            entry_id: playing.value.current.clone().unwrap(),
        },
    ] {
        assert!(matches!(
            client.fm_command("1", FmSource::Personal, command).await,
            Err(Error::QueueChanged)
        ));
    }
    assert_eq!(api.count("/api/radio/trash/add"), 0);
    assert!(
        client
            .fm_view("1", familiar, false)
            .await
            .unwrap()
            .value
            .active
    );
}
#[tokio::test]
async fn native_fm_projection_player_actions_and_recovery_are_connected() {
    let (_dir, api, _session, client) = fixture();
    let mut song = domain::song(1);
    song["cookie"] = json!("PRIVATE");
    song["mp3Url"] = json!("https://private/?secret=PRIVATE");
    api.batches.lock().unwrap().push_back(vec![
        song,
        domain::song(2),
        domain::song(3),
        domain::song(4),
    ]);
    let loaded = client.fm_view("1", FmSource::Personal, true).await.unwrap();
    assert_eq!(loaded.value.items.len(), 4);
    assert!(!loaded.value.active);
    let safe = serde_json::to_string(&loaded).unwrap();
    assert!(!safe.contains("PRIVATE") && !safe.contains("alg-"));
    let started = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert!(started.value.active);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert!(next.fm);
    assert_eq!(next.track.unwrap().id, "2");
    assert!(
        client
            .player_action("1", PlayerAction::Clear)
            .await
            .is_err()
    );
    assert!(
        client
            .player_action(
                "1",
                PlayerAction::Mode {
                    mode: PlaybackMode::Random
                }
            )
            .await
            .is_err()
    );
    client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::SingleLoop,
            },
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert!(queue.fm);
    let stale_entry = started.value.current.unwrap();
    assert!(matches!(
        client
            .fm_command(
                "1",
                FmSource::Personal,
                FmCommand::Trash {
                    entry_id: stale_entry
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.count("/api/radio/trash/add"), 0);
    let entry = queue.current.unwrap();
    api.fault("/api/radio/trash/add", domain::Fault::Lost);
    assert!(
        client
            .fm_command(
                "1",
                FmSource::Personal,
                FmCommand::Trash { entry_id: entry }
            )
            .await
            .is_err()
    );
    let uncertain = client
        .fm_view("1", FmSource::Personal, false)
        .await
        .unwrap();
    let op = uncertain
        .value
        .operations
        .iter()
        .find(|o| o.kind == "trash")
        .unwrap();
    assert_eq!(op.phase, "uncertain");
    let settled = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Reconcile { id: op.id.clone() },
        )
        .await
        .unwrap();
    assert!(settled.value.operations.iter().all(|o| o.kind != "trash"));
    assert_eq!(api.count("/api/radio/trash/add"), 1);
    client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::RestoreTrash {
                ids: vec!["2".into()],
            },
        )
        .await
        .unwrap();
    let trash = client.fm_trash("1", 1, json!("")).await.unwrap();
    assert!(trash.value.songs.is_empty());
    let left = client
        .fm_command("1", FmSource::Personal, FmCommand::Leave)
        .await
        .unwrap();
    assert!(!left.value.active);
}
#[tokio::test]
async fn fm_and_together_do_not_enter_each_others_queue_and_stale_account_does_not_send() {
    let (_dir, api, session, client) = fixture();
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
    assert!(matches!(
        client.together_command("1", TogetherCommand::Create).await,
        Err(Error::State("leave FM before entering together"))
    ));
    assert_eq!(api.count("/api/listen/together/room/create"), 0);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let count = api.calls.lock().unwrap().len();
    assert!(matches!(
        client.fm_view("1", FmSource::Personal, true).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .fm_command(
                "1",
                FmSource::Personal,
                FmCommand::RestoreTrash {
                    ids: vec!["1".into()]
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.calls.lock().unwrap().len(), count);
    let other = client
        .fm_view("2", FmSource::Personal, false)
        .await
        .unwrap();
    assert!(other.value.items.is_empty());
    assert!(other.value.operations.is_empty());
}

#[tokio::test]
async fn empty_recommendations_keep_leave_and_together_guards_until_explicit_exit() {
    let (_dir, api, _session, client) = fixture();
    let empty = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert!(empty.value.active);
    assert!(empty.value.items.is_empty());
    assert!(matches!(
        client.together_command("1", TogetherCommand::Create).await,
        Err(Error::State("leave FM before entering together"))
    ));
    let resumed = client
        .fm_command("1", FmSource::Personal, FmCommand::Resume)
        .await
        .unwrap();
    assert!(resumed.value.active);
    let left = client
        .fm_command("1", FmSource::Personal, FmCommand::Leave)
        .await
        .unwrap();
    assert!(!left.value.active);
    assert_eq!(api.count("/api/v1/radio/skip"), 0);
}

#[tokio::test]
async fn native_reason_dialog_artist_state_and_explicit_restore_share_account_epoch() {
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3, 4]);
    let initial = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let entry = initial.value.current.unwrap();
    assert!(
        client
            .fm_reasons("0", FmSource::Personal, &entry)
            .await
            .is_err()
    );
    let choices = client
        .fm_reasons("1", FmSource::Personal, &entry)
        .await
        .unwrap();
    assert!(
        client
            .fm_command(
                "1",
                FmSource::Personal,
                FmCommand::Feedback {
                    entry_id: entry.clone(),
                    reason_key: "invented".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(api.count("/api/radio/trash/add"), 0);
    let reason = choices
        .value
        .options
        .into_iter()
        .find(|r| r.artist_id.is_some())
        .unwrap();
    api.fault("/api/music-blacklist/add", domain::Fault::Reject(403));
    assert!(
        client
            .fm_command(
                "1",
                FmSource::Personal,
                FmCommand::Feedback {
                    entry_id: entry,
                    reason_key: reason.key
                }
            )
            .await
            .is_err()
    );
    let failed = client
        .fm_view("1", FmSource::Personal, false)
        .await
        .unwrap();
    assert!(
        failed
            .value
            .operations
            .iter()
            .any(|op| op.kind == "artist" && op.phase == "rejected")
    );
    let blocked = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Artist {
                id: "1".into(),
                blocked: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(blocked.value.blocked_artists[0].name, "fixture artist");
    assert!(
        blocked
            .value
            .operations
            .iter()
            .all(|op| op.kind != "artist")
    );
    let restored = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Artist {
                id: "1".into(),
                blocked: false,
            },
        )
        .await
        .unwrap();
    assert!(restored.value.blocked_artists.is_empty());
    assert_eq!(api.count("/api/radio/trash/add"), 1);
}

#[tokio::test]
async fn artist_readback_filters_a_current_song_that_became_blocked_after_lost_reply() {
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 4, 2, 3]);
    let start = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let entry = start.value.current.unwrap();
    let reasons = client
        .fm_reasons("1", FmSource::Personal, &entry)
        .await
        .unwrap();
    let key = reasons
        .value
        .options
        .iter()
        .find(|r| r.artist_id.is_some())
        .unwrap()
        .key
        .clone();
    api.fault("/api/music-blacklist/add", domain::Fault::Lost);
    assert!(
        client
            .fm_command(
                "1",
                FmSource::Personal,
                FmCommand::Feedback {
                    entry_id: entry,
                    reason_key: key
                }
            )
            .await
            .is_err()
    );
    let pending = client
        .fm_view("1", FmSource::Personal, false)
        .await
        .unwrap();
    assert_eq!(
        pending
            .value
            .items
            .iter()
            .find(|r| Some(&r.entry_id) == pending.value.current.as_ref())
            .unwrap()
            .track
            .id,
        "4"
    );
    let op = pending
        .value
        .operations
        .iter()
        .find(|r| r.kind == "artist")
        .unwrap();
    let settled = client
        .fm_command(
            "1",
            FmSource::Personal,
            FmCommand::ReconcileArtist { id: op.id.clone() },
        )
        .await
        .unwrap();
    assert_eq!(
        settled
            .value
            .items
            .iter()
            .find(|r| Some(&r.entry_id) == settled.value.current.as_ref())
            .unwrap()
            .track
            .id,
        "2"
    );
    assert_eq!(api.count("/api/v1/radio/skip"), 0);
    assert_eq!(api.count("/api/music-blacklist/add"), 1);
}

#[tokio::test]
async fn native_menu_steps_and_resumes_through_fm_coordinator() {
    use cumulus_core::client::native_media::NativeMediaAction as Action;
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3, 4]);
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
    for (action, id) in [
        (Action::Next, "2"),
        (Action::Previous, "1"),
        (Action::Pause, "1"),
        (Action::Play, "1"),
    ] {
        // The recovered FM coordinator deliberately rejects steps within 250 ms.
        if action == Action::Previous {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        let target = client.native_media_snapshot().unwrap().target.unwrap();
        client.native_media_action(&target, action).await.unwrap();
        let player = client
            .subscribe_playback("1")
            .await
            .unwrap()
            .current()
            .unwrap();
        assert!(player.fm);
        assert_eq!(player.track.unwrap().id, id);
    }
    assert_eq!(api.count("/api/radio/trash/add"), 0);
}

#[tokio::test]
async fn system_fm_steps_preserve_pause_and_stop_through_the_coordinator() {
    use cumulus_core::{client::native_media::NativeMediaStatus as Status, playback::Step};
    let (_dir, api, _session, client) = fixture();
    api.batch(&[1, 2, 3, 4]);
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
    let target = client.native_media_snapshot().unwrap().target.unwrap();
    client.native_media_step(&target, Step::Next).await.unwrap();
    assert_eq!(client.native_media_details().unwrap().state, Status::Paused);
    let view = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(view.fm);
    assert_eq!(view.track.unwrap().id, "2");
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let target = client.native_media_snapshot().unwrap().target.unwrap();
    client
        .native_media_step(&target, Step::Previous)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        client.native_media_details().unwrap().state,
        Status::Stopped
    );
    let view = client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(view.fm);
    assert_eq!(view.track.unwrap().id, "1");
    assert_eq!(api.count("/api/radio/trash/add"), 0);
}
