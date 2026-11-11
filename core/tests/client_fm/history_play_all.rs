use super::{domain, fixture};
use cumulus_core::{
    Error,
    client::dto::{FmCommand, FmSource, PlaySelection, PlayerAction},
    history::{HistoryKind, HistoryRecord, HistoryService},
    membership::Quality,
    playback::{PlaybackMode, controller::PlaybackStatus},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::time::Duration;

const DETAILS: &str = "/api/v3/song/detail";
const RIGHTS: &str = "/api/song/enhance/privilege";
const PLAY_URL: &str = "/api/song/enhance/player/url/v1";

fn song(id: &str) -> Value {
    json!({"id":id,"name":format!("history song {id}"),"dt":120000})
}

fn catalogue(api: &domain::Backend, ids: &[&str], eligible: &[&str]) {
    let privileges: Vec<_> = ids
        .iter()
        .map(|id| {
            let allowed = eligible.contains(id);
            json!({"id":id,"pl":if allowed {128000} else {0},"st":if allowed {0} else {-100},"fee":0})
        })
        .collect();
    let mut responses = api.responses.lock().unwrap();
    responses.insert(
        DETAILS.into(),
        json!({"code":200,"songs":ids.iter().map(|id|song(id)).collect::<Vec<_>>(),"privileges":privileges}),
    );
    responses.insert(RIGHTS.into(), json!({"code":200,"data":privileges}));
}

#[tokio::test]
async fn history_play_all_keeps_fm_on_rejection_then_replaces_it_with_the_prior_ordinary_mode() {
    let (dir, api, session, client) = fixture();
    let epoch = "1";
    let store = StateStore::open(dir.path()).unwrap();
    let read = |namespace, key| store.get("9", namespace, key).unwrap();
    let queue_ids = |value: &Value| {
        value["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["resource"]["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };

    catalogue(&api, &["90", "91"], &["90", "91"]);
    client
        .play_selection(
            epoch,
            PlaySelection::Songs {
                ids: vec!["90".into(), "91".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    client
        .player_action(
            epoch,
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    client
        .player_action(epoch, PlayerAction::Seek { seconds: 12.5 })
        .await
        .unwrap();

    api.batch(&[1, 2, 3, 4]);
    let fm = client
        .fm_command(
            epoch,
            FmSource::Personal,
            FmCommand::Start {
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(fm.value.active_source, Some(FmSource::Personal));
    let playing_fm = client
        .player_action(epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert!(playing_fm.fm);
    assert_eq!(playing_fm.state, PlaybackStatus::Paused);
    assert_eq!(playing_fm.track.as_ref().unwrap().id, "1");
    assert_ne!(playing_fm.mode, PlaybackMode::Random);

    let fm_queue = read("playback", "queue").unwrap();
    let backup_queue = read("playback", "fm-backup-queue").unwrap();
    let backup_position = read("playback", "fm-backup-position").unwrap();
    let fm_source = read("fm-player-v1", "source").unwrap();
    assert_eq!(queue_ids(&backup_queue), ["90", "91"]);
    assert_eq!(backup_queue["mode"], "random");

    let kind = HistoryKind::Song;
    let history = HistoryService::new(session.clone()).unwrap();
    for (id, at) in [("31", 500), ("32", 400)] {
        history
            .record_for(
                &session.token().unwrap(),
                kind,
                HistoryRecord::local(kind, song(id), at).unwrap(),
            )
            .unwrap();
    }
    let page = client.history_page(epoch, kind, false).await.unwrap().value;
    let ids: Vec<_> = page.entries.iter().map(|row| row.id.as_str()).collect();
    let replace = || PlaySelection::HistoryPlayAll {
        history_kind: kind,
        revision: page.revision.clone(),
        quality: Quality::Standard,
    };
    let ticket = || {
        client
            .prepare_play_selection(
                epoch,
                &playing_fm.queue_revision,
                playing_fm.play_id.as_deref(),
            )
            .unwrap()
    };

    // Reject on fresh rights, after preparation rather than on an invalid
    // receipt. The FM decoder and its durable restore material must survive.
    catalogue(&api, &ids, &[]);
    let no_new_url = || {
        api.calls.lock().unwrap().iter().all(|(path, args)| {
            path != PLAY_URL
                || serde_json::from_str::<Vec<Value>>(args["ids"].as_str().unwrap())
                    .unwrap()
                    .iter()
                    .all(|id| !matches!(id.as_str(), Some("31" | "32")))
        })
    };
    let right_reads = api.count(RIGHTS);
    assert!(matches!(
        client
            .commit_prepared_selection(epoch, &ticket(), replace())
            .await,
        Err(Error::State("history selection has no playable resources"))
    ));
    assert_eq!(api.count(RIGHTS), right_reads + 1);
    assert!(no_new_url());
    let still_fm = client.player_snapshot(epoch).await.unwrap();
    assert!(still_fm.fm);
    assert_eq!(still_fm.state, playing_fm.state);
    assert_eq!(still_fm.mode, playing_fm.mode);
    assert_eq!(still_fm.play_id, playing_fm.play_id);
    assert_eq!(still_fm.entry_id, playing_fm.entry_id);
    assert_eq!(still_fm.position, playing_fm.position);
    assert_eq!(still_fm.queue_revision, playing_fm.queue_revision);
    assert_eq!(read("playback", "queue"), Some(fm_queue));
    assert_eq!(
        read("playback", "fm-backup-queue"),
        Some(backup_queue.clone())
    );
    assert_eq!(
        read("playback", "fm-backup-position"),
        Some(backup_position)
    );
    assert_eq!(read("fm-player-v1", "source"), Some(fm_source));

    // A new explicit Play All keeps native history order, removes both FM
    // and backed-up songs, and restores only the prior ordinary mode.
    catalogue(&api, &ids, &["31", "32"]);
    *api.hold.lock().unwrap() = Some(DETAILS.into());
    let worker = client.clone();
    let (receipt, input) = (ticket(), replace());
    let pending = tokio::spawn(async move {
        worker
            .commit_prepared_selection(epoch, &receipt, input)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), api.started.notified())
        .await
        .unwrap();
    let mut changed = backup_queue.clone();
    changed["mode"] = json!("single_loop");
    store
        .put("9", "playback", "fm-backup-queue", &changed)
        .unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    let held = client.player_snapshot(epoch).await.unwrap();
    assert_eq!(held.queue_revision, playing_fm.queue_revision);
    assert_eq!(held.play_id, playing_fm.play_id);
    assert_eq!(read("playback", "fm-backup-queue"), Some(changed));
    assert!(no_new_url());
    store
        .put("9", "playback", "fm-backup-queue", &backup_queue)
        .unwrap();
    let started = client
        .commit_prepared_selection(epoch, &ticket(), replace())
        .await
        .unwrap();
    assert!(!started.fm);
    assert_eq!(started.state, PlaybackStatus::Playing);
    assert_eq!(started.mode, PlaybackMode::Random);
    assert!(matches!(started.track.unwrap().id.as_str(), "31" | "32"));
    assert_ne!(started.play_id, playing_fm.play_id);
    let queue = client.player_queue(epoch).await.unwrap();
    assert!(!queue.fm);
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        ["31", "32"]
    );
    assert_eq!(queue_ids(&read("playback", "queue").unwrap()), ["31", "32"]);
    assert!(read("playback", "fm-backup-queue").is_none());
    assert!(read("playback", "fm-backup-position").is_none());
    assert!(read("fm-player-v1", "source").is_none());
    assert_eq!(api.count("/api/v1/radio/skip"), 0);
}
