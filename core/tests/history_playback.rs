#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    history::{HistoryKind as Kind, HistoryRecord},
    membership::Quality,
    playback::controller::{PlaybackCommand as Cmd, PlaybackStatus as Status},
};
use serde_json::json;
use std::{sync::atomic::Ordering, time::Duration};
use support::*;

fn rows(f: &Fixture, owner: &str, kind: Kind) -> Vec<HistoryRecord> {
    f.store
        .get(owner, "play-history-v1", &format!("local:{}", kind.key()))
        .unwrap()
        .map(|v| serde_json::from_value(v).unwrap())
        .unwrap_or_default()
}
fn album() -> Cmd {
    Cmd::Replace {
        resources: vec![json!({"id":1,"dt":120000,"song":{"id":1,"name":"Song",
            "al":{"id":7,"name":"Album","picUrl":"https://p1.music.126.net/album.jpg"},
            "ar":[{"id":3,"name":"Artist"}],"dt":120000}})],
        index: 0,
        source: json!({"kind":"album","id":"7"}),
    }
}

#[tokio::test]
async fn successful_play_records_source_and_song_once_then_reopen_waits_for_actual_resume() {
    let f = Fixture::new();
    let playing = f.command(album()).await.unwrap();
    assert_eq!(playing.state, Status::Playing);
    assert!(playing.history_error.is_none());
    let first = rows(&f, "9", Kind::Song);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, "1");
    assert_eq!(first[0].resource["name"], "Song");
    let albums = rows(&f, "9", Kind::Album);
    assert_eq!(albums[0].id, "7");
    assert_eq!(albums[0].resource["artists"][0]["name"], "Artist");
    assert_eq!(albums[0].played_at, first[0].played_at);
    f.command(Cmd::Seek(20.0)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Pause(false)).await.unwrap();
    f.command(Cmd::Quality(Quality::Lossless)).await.unwrap();
    f.command(Cmd::RefreshSource).await.unwrap();
    assert_eq!(rows(&f, "9", Kind::Song), first);
    f.command(Cmd::Stop).await.unwrap();
    let restarted = f.another_player();
    let state = restarted.command(&f.token(), Cmd::Restore).await.unwrap();
    assert_eq!(state.state, Status::Paused);
    assert_eq!(rows(&f, "9", Kind::Song), first);
    restarted
        .command(&f.token(), Cmd::Seek(30.0))
        .await
        .unwrap();
    assert_eq!(rows(&f, "9", Kind::Song), first);
    restarted.command(&f.token(), Cmd::Play).await.unwrap();
    assert!(rows(&f, "9", Kind::Song)[0].played_at > first[0].played_at);
    restarted.command(&f.token(), Cmd::Stop).await.unwrap();
    assert_eq!(rows(&f, "9", Kind::Album).len(), 1);
}

#[tokio::test]
async fn failed_or_cancelled_engine_start_cannot_create_a_history_record() {
    let f = Fixture::new();
    f.device.fail_unpause.store(true, Ordering::SeqCst);
    assert!(f.command(replace(&[1])).await.is_err());
    assert_eq!(f.player.snapshot().state, Status::Error);
    assert!(rows(&f, "9", Kind::Song).is_empty());
    // Use a fresh device so the earlier completed load's Notify permit cannot
    // be mistaken for this second load actually reaching the decoder.
    let f = Fixture::new();
    f.device.hold_load.store(true, Ordering::SeqCst);
    let loading = f.start(replace(&[2]));
    tokio::time::timeout(Duration::from_secs(4), f.device.started.notified())
        .await
        .unwrap();
    f.command(Cmd::Stop).await.unwrap();
    assert!(loading.await.unwrap().is_err());
    f.device.hold_load.store(false, Ordering::SeqCst);
    f.command(replace(&[3])).await.unwrap();
    let songs = rows(&f, "9", Kind::Song);
    assert_eq!(
        songs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["3"]
    );
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn session_change_during_decode_cannot_publish_to_either_account() {
    let f = Fixture::new();
    f.device.hold_load.store(true, Ordering::SeqCst);
    let loading = f.start(replace(&[1]));
    f.device.started.notified().await;
    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &f.token())
        .unwrap();
    until(&f.player, |s| s.account.uid.as_deref() == Some("10")).await;
    assert!(loading.await.unwrap().is_err());
    f.device.hold_load.store(false, Ordering::SeqCst);
    assert!(rows(&f, "9", Kind::Song).is_empty());
    assert!(rows(&f, "10", Kind::Song).is_empty());
    f.command(replace(&[2])).await.unwrap();
    assert_eq!(rows(&f, "10", Kind::Song)[0].id, "2");
    assert!(rows(&f, "9", Kind::Song).is_empty());
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn atomic_history_failure_is_nonfatal_and_retries_without_reloading_audio() {
    let f = Fixture::new();
    f.sql(
        "CREATE TRIGGER reject_history BEFORE INSERT ON state
        WHEN NEW.kind='play-history-v1' AND NEW.key='local:album'
        BEGIN SELECT RAISE(ABORT, 'fixture history failure'); END;",
    );
    let playing = f.command(album()).await.unwrap();
    assert_eq!(playing.state, Status::Playing);
    assert!(playing.error.is_none());
    assert!(playing.history_error.is_some());
    assert!(rows(&f, "9", Kind::Song).is_empty()); // same transaction as album
    assert!(rows(&f, "9", Kind::Album).is_empty());
    f.sql("DROP TRIGGER reject_history;");
    until(&f.player, |s| {
        s.state == Status::Playing && s.history_error.is_none()
    })
    .await;
    assert_eq!(rows(&f, "9", Kind::Song).len(), 1);
    assert_eq!(rows(&f, "9", Kind::Album).len(), 1);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
    assert_eq!(f.api.count(1), 1);
    f.command(Cmd::Stop).await.unwrap();
}

#[tokio::test]
async fn deletion_and_failed_checkpoint_retry_cannot_resurrect_removed_history() {
    let f = Fixture::new();
    f.command(replace(&[1])).await.unwrap();
    f.store
        .put("9", "play-history-v1", "local:song", &json!([]))
        .unwrap();
    f.command(Cmd::Seek(25.0)).await.unwrap();
    f.command(Cmd::Pause(true)).await.unwrap();
    f.command(Cmd::Pause(false)).await.unwrap();
    assert!(rows(&f, "9", Kind::Song).is_empty());
    f.command(Cmd::Stop).await.unwrap();
    f.sql(
        "CREATE TRIGGER reject_history BEFORE INSERT ON state WHEN NEW.kind='play-history-v1'
        BEGIN SELECT RAISE(ABORT, 'fixture history failure'); END;",
    );
    assert!(
        f.command(replace(&[2]))
            .await
            .unwrap()
            .history_error
            .is_some()
    );
    f.sql("DROP TRIGGER reject_history;");
    // Same tombstone written atomically by a user clear in HistoryService.
    let at = (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as u64;
    f.store
        .put("9", "play-history-v1", "cleared:song", &json!(at))
        .unwrap();
    f.command(Cmd::Seek(30.0)).await.unwrap();
    assert!(f.player.snapshot().history_error.is_none());
    assert!(rows(&f, "9", Kind::Song).is_empty());
    tokio::time::sleep(Duration::from_millis(5)).await;
    f.command(replace(&[2])).await.unwrap();
    assert_eq!(rows(&f, "9", Kind::Song)[0].id, "2"); // explicit new playback
    f.command(Cmd::Stop).await.unwrap();
}
