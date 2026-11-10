use super::{ASSIGNMENT_PATH, playback, vinyl_http};
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{MusicClient, dto::PlayerAction, pc_experiments::OrdinarySkipAssignment},
    playback::{PlaybackMode, PlaybackQueue, Step},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

fn song(id: u64, playable: bool) -> Value {
    json!({"id":id,"song":{"id":id,"name":format!("合成 {id}"),"dt":120000,
        "privilege":{"fee":0,"flag":0,"pl":if playable {128000} else {0},
            "st":if playable {0} else {-200}}}})
}

fn url(id: u64) -> Value {
    json!({"code":200,"data":[{"id":id,"code":200,"url":"https://m1.music.126.net/fixture.mp3",
        "md5":"abc","level":"standard","expi":120}]})
}

async fn run(browser: Option<bool>, group: &str, expected: u64) {
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(
        root.path(),
        &["MUSIC_U=synthetic"],
        [
            url(1).into(),
            json!({"code":200,"data":[{"expName":"PH-PC-skipSongs2","expGroupName":group}]}).into(),
            json!({"code":200,"msg":0,"comment":0,"forward":0,"notice":0}).into(),
        ]
        .into_iter()
        .chain((expected != 1).then(|| url(expected).into())),
    );
    let registry = super::fixture_registry(&root.path().join("host"));
    let mut queue = PlaybackQueue::new(Some(registry.store()), "71").unwrap();
    // Manual stepping still advances in single-repeat mode. Its automatic
    // prefetch stays on the current (already resolved) song, so it does not
    // consume the explicit next request in this sealed wire transcript.
    queue.set_mode(PlaybackMode::SingleLoop).unwrap();
    queue
        .replace(
            vec![song(1, true), song(2, false), song(3, true)],
            0,
            json!({"kind":"playlist"}),
        )
        .unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session, registry).with_audio_factory(|| {
        Ok(playback::audio_engine(
            Arc::new(playback::Device::default()),
        ))
    });
    let before = client
        .player_action(&epoch, PlayerAction::Play)
        .await
        .unwrap();
    let assignment = client
        .official_pc_ordinary_skip_assignment(&epoch)
        .await
        .unwrap();
    assert_eq!(
        assignment,
        if group == "t1" {
            OrdinarySkipAssignment::Enabled
        } else {
            OrdinarySkipAssignment::Control
        }
    );
    let poll = match browser {
        Some(online) => client.start_pc_message_connectivity(online),
        None => client.start_pc_message_count_polling(),
    }
    .unwrap();
    let mut counts = poll.subscribe();
    tokio::time::timeout(Duration::from_secs(3), async {
        while counts.borrow_and_update().counts.is_none() {
            counts.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let action = || PlayerAction::StepMiniBar {
        play_id: before.play_id.clone().unwrap(),
        expected_queue_revision: before.queue_revision.clone(),
        direction: Step::Next,
    };
    let mut notices = client.subscribe_ordinary_skip(&epoch).unwrap();
    let after = client.player_action(&epoch, action()).await.unwrap();
    if expected == 3 {
        let notice = tokio::time::timeout(Duration::from_secs(2), notices.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(notice.epoch, epoch);
        assert_eq!(notice.id.len(), 32);
        assert!((1..=3000).contains(&notice.remaining_ms));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(60), notices.next())
            .await
            .is_err()
    );
    assert_eq!(after.track.unwrap().id, expected.to_string());
    if expected == 1 {
        assert_eq!(before.play_id, after.play_id);
        assert_eq!(before.entry_id, after.entry_id);
    } else {
        assert_ne!(before.play_id, after.play_id);
        assert!(
            matches!(client.player_action(&epoch, action()).await, Err(error) if matches!(error.root(), Error::StaleResponse))
        );
    }
    let calls = http.calls();
    let mut expected_paths = vec![
        "/api/song/enhance/player/url/v1",
        ASSIGNMENT_PATH,
        "/api/pl/count",
    ];
    if expected != 1 {
        expected_paths.push("/api/song/enhance/player/url/v1");
    }
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        expected_paths
    );
    if let Some(next) = calls.get(3) {
        let ids: Vec<Value> = serde_json::from_str(next.payload["ids"].as_str().unwrap()).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(
            cumulus_core::protocol::api::identifier(&ids[0]).unwrap(),
            expected.to_string()
        );
    }
    poll.shutdown().await;
}

#[tokio::test]
async fn signed_main_minibar_uses_denial_scan_only_with_current_online_t1() {
    run(Some(true), "t1", 3).await;
    run(None, "t1", 2).await;
    run(Some(false), "t1", 2).await;
    run(Some(true), "c", 1).await;
}
