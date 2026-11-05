use super::{
    ASSIGNMENT_PATH, playback,
    support::http::{HttpResponse, ScriptHttp, gate, reply},
    vinyl_http,
};
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlayerAction, PlayerView},
        messages::PcMessageConnectivity,
    },
    playback::{
        PlaybackMode, PlaybackQueue,
        controller::PlaybackStatus,
        engine::{EndReason, EngineEvent},
    },
    protocol::{api::identifier, http::ApiOrigin},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const AUTH: &str = "/api/song/enhance/player/url/v1";

fn song(id: u64, playable: bool) -> Value {
    json!({"id":id,"song":{"id":id,"name":format!("合成 {id}"),"dt":120000,
        "privilege":{"fee":0,"flag":0,"pl":if playable {128000} else {0},
            "st":if playable {0} else {-200}}}})
}
fn url(id: u64, allowed: bool) -> Value {
    if allowed {
        json!({"code":200,"data":[{"id":id,"code":200,
            "url":format!("https://m1.music.126.net/fixture-{id}.mp3"),
            "md5":"abc","level":"standard","expi":120}]})
    } else {
        json!({"code":200,"data":[{"id":id,"code":403}]})
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    client: MusicClient,
    poll: PcMessageConnectivity,
    store: Arc<StateStore>,
    wire: Arc<ScriptHttp>,
    device: Arc<playback::Device>,
    epoch: String,
}

impl Fixture {
    async fn start(
        rows: Vec<Value>,
        mode: PlaybackMode,
        group: &str,
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> (Self, PlayerView) {
        let root = tempfile::tempdir().unwrap();
        let (api, wire) = vinyl_http(
            root.path(),
            &["MUSIC_U=synthetic"],
            [
                json!({"code":200,"data":[{"expName":"PH-PC-skipSongs2","expGroupName":group}]})
                    .into(),
                json!({"code":200,"msg":0,"comment":0,"forward":0,"notice":0}).into(),
                url(1, true).into(),
            ]
            .into_iter()
            .chain(replies),
        );
        let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let mut queue = PlaybackQueue::new(Some(store.clone()), "71").unwrap();
        queue.set_mode(mode).unwrap();
        queue.replace(rows, 0, json!({"kind":"playlist"})).unwrap();
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api, json!({"userId":71}), &session.token().unwrap())
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let device = Arc::new(playback::Device::default());
        let factory = device.clone();
        let client = MusicClient::with_session(session, registry)
            .with_audio_factory(move || Ok(playback::audio_engine(factory.clone())));
        client
            .official_pc_ordinary_skip_assignment(&epoch)
            .await
            .unwrap();
        let poll = client.start_pc_message_connectivity(true).unwrap();
        let mut counts = poll.subscribe();
        tokio::time::timeout(Duration::from_secs(3), async {
            while counts.borrow_and_update().counts.is_none() {
                counts.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let before = client
            .player_action(&epoch, PlayerAction::Play)
            .await
            .unwrap();
        (
            Self {
                _root: root,
                client,
                poll,
                store,
                wire,
                device,
                epoch,
            },
            before,
        )
    }

    fn native_entry(&self) -> i64 {
        self.device.state.lock().unwrap().view.entry_id.unwrap()
    }
    fn loads(&self) -> usize {
        self.device.state.lock().unwrap().loads
    }
    fn event(&self, entry_id: i64, reason: EndReason, error: i32) {
        self.device
            .state
            .lock()
            .unwrap()
            .view
            .events
            .push(EngineEvent::Ended {
                entry_id,
                reason,
                error,
            });
    }
    fn eof(&self) {
        self.event(self.native_entry(), EndReason::Eof, 0);
    }
    async fn view(&self) -> PlayerView {
        self.client.player_snapshot(&self.epoch).await.unwrap()
    }
    async fn wait(&self, condition: impl Fn(&PlayerView) -> bool) -> PlayerView {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let view = self.view().await;
                if condition(&view) {
                    return view;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        })
        .await
        .expect("native EOF fixture did not reach the expected state")
    }
    fn assert_wire(&self, ids: &[&str]) {
        let calls = self.wire.calls();
        assert_eq!(calls.len(), ids.len() + 2);
        assert_eq!(calls[0].path, ASSIGNMENT_PATH);
        assert_eq!(calls[1].path, "/api/pl/count");
        for (call, expected) in calls[2..].iter().zip(ids) {
            assert_eq!(call.path, AUTH);
            let packed: Vec<Value> =
                serde_json::from_str(call.payload["ids"].as_str().unwrap()).unwrap();
            assert_eq!(packed.len(), 1);
            assert_eq!(identifier(&packed[0]).unwrap(), *expected);
        }
    }
    fn assert_durable_end(&self, entry: Option<&str>) {
        let position = self
            .store
            .get("71", "playback", "position")
            .unwrap()
            .unwrap();
        assert_eq!(position["entry_id"].as_str(), entry);
        assert_eq!(position["state"], "ended");
        assert_eq!(position["seconds"].as_f64(), Some(120.0));
    }
}

#[tokio::test]
async fn signed_eof_skips_only_after_the_independent_adjacent_prefetch() {
    let (f, before) = Fixture::start(
        vec![song(1, true), song(2, false), song(3, true)],
        PlaybackMode::Sequential,
        "t1",
        [url(2, false).into(), url(3, true).into()],
    )
    .await;
    f.wire.wait(4).await;
    f.assert_wire(&["1", "2"]); // admitted as preload, never as a selected decoder
    let queue = PlaybackQueue::new(Some(f.store.clone()), "71")
        .unwrap()
        .snapshot();
    f.eof();
    let after = f
        .wait(|view| view.state == PlaybackStatus::Playing && view.play_id != before.play_id)
        .await;
    assert_eq!(after.track.unwrap().id, "3");
    assert_ne!(before.entry_id, after.entry_id);
    assert_eq!(f.loads(), 2);
    let current = PlaybackQueue::new(Some(f.store.clone()), "71")
        .unwrap()
        .snapshot();
    assert_eq!(current.order, queue.order);
    assert_eq!(current.items, queue.items);
    assert_eq!(current.current, Some(queue.items[2].entry_id.clone()));
    f.assert_wire(&["1", "2", "3"]);
    f.poll.shutdown().await;
}

#[tokio::test]
async fn signed_denied_tail_stops_on_finished_row_but_control_keeps_finished_identity() {
    for (group, expected) in [("t1", PlaybackStatus::Idle), ("c", PlaybackStatus::Ended)] {
        let (f, before) = Fixture::start(
            vec![song(1, true), song(2, false), song(3, false)],
            PlaybackMode::Sequential,
            group,
            [url(2, false).into()],
        )
        .await;
        f.wire.wait(4).await;
        f.eof();
        let after = f.wait(|view| view.state == expected).await;
        assert_eq!(after.entry_id, before.entry_id);
        assert_eq!(after.queue_revision, before.queue_revision);
        assert_eq!(after.track.as_ref().unwrap().id, "1");
        assert_eq!(after.position, Some(120.0));
        assert!(after.error.is_none());
        if group == "t1" {
            assert!(after.play_id.is_none());
        } else {
            assert_eq!(after.play_id, before.play_id);
            // No new branch should subsequently leave the finished row.
            let unexpected = tokio::time::timeout(
                Duration::from_millis(300),
                f.wait(|view| {
                    view.state != PlaybackStatus::Ended || view.entry_id != before.entry_id
                }),
            )
            .await;
            assert!(unexpected.is_err());
            let clicked = f
                .client
                .player_action(
                    &f.epoch,
                    PlayerAction::StepMiniBar {
                        play_id: after.play_id.unwrap(),
                        expected_queue_revision: after.queue_revision.clone(),
                        direction: cumulus_core::playback::Step::Next,
                    },
                )
                .await
                .unwrap();
            assert_eq!(clicked.state, PlaybackStatus::Ended);
            assert_eq!(clicked.entry_id, before.entry_id);
            assert_eq!(clicked.play_id, before.play_id);
            assert_eq!(clicked.queue_revision, before.queue_revision);
            assert_eq!(clicked.position, Some(120.0));
        }
        f.assert_durable_end(before.entry_id.as_deref());
        assert_eq!(f.loads(), 1);
        f.assert_wire(&["1", "2"]);
        f.poll.shutdown().await;
    }
}

#[tokio::test]
async fn stale_native_entry_and_error_are_not_copyright_eof() {
    let (f, before) = Fixture::start(
        vec![song(1, true), song(2, false), song(3, true)],
        PlaybackMode::Sequential,
        "t1",
        [url(2, false).into()],
    )
    .await;
    f.wire.wait(4).await;
    let native = f.native_entry();
    let before_polls = {
        let mut device = f.device.state.lock().unwrap();
        device.view.events.push(EngineEvent::Ended {
            entry_id: native + 10_000,
            reason: EndReason::Eof,
            error: 0,
        });
        device.polls
    };
    let same = f
        .wait(|_| f.device.state.lock().unwrap().polls > before_polls)
        .await;
    assert_eq!(same.play_id, before.play_id);
    assert_eq!(same.state, PlaybackStatus::Playing);
    // Even a success event drained in the same poll must not win.
    {
        let mut device = f.device.state.lock().unwrap();
        for error in [0, -13] {
            device.view.events.push(EngineEvent::Ended {
                entry_id: native,
                reason: EndReason::Eof,
                error,
            });
        }
    }
    let failed = f.wait(|view| view.state == PlaybackStatus::Error).await;
    assert_eq!(failed.entry_id, before.entry_id);
    assert!(failed.error.is_some());
    assert_eq!(f.loads(), 1);
    f.assert_wire(&["1", "2"]);
    f.poll.shutdown().await;
}

#[tokio::test]
async fn signed_eof_target_source_denial_is_not_another_skip() {
    let (f, _) = Fixture::start(
        vec![song(1, true), song(2, false), song(3, true), song(4, true)],
        PlaybackMode::Sequential,
        "t1",
        [url(2, false).into(), url(3, false).into()],
    )
    .await;
    f.wire.wait(4).await;
    f.eof();
    let after = f.wait(|view| view.state == PlaybackStatus::Error).await;
    assert_eq!(after.track.unwrap().id, "3");
    assert!(after.error.is_some());
    assert_eq!(f.loads(), 1);
    f.assert_wire(&["1", "2", "3"]);
    f.poll.shutdown().await;
}

#[tokio::test]
async fn new_user_stop_retires_a_held_eof_target_before_it_can_decode() {
    let (held, release) = gate();
    let (f, _) = Fixture::start(
        vec![song(1, true), song(2, false), song(3, true)],
        PlaybackMode::Sequential,
        "t1",
        [url(2, false).into(), held],
    )
    .await;
    f.wire.wait(4).await;
    f.eof();
    f.wire.wait(5).await;
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    let _ = release.send(Ok(reply(url(3, true))));
    let after = f.view().await;
    assert_eq!(after.state, PlaybackStatus::Idle);
    assert!(after.play_id.is_none());
    assert_eq!(f.loads(), 1);
    f.assert_wire(&["1", "2", "3"]);
    f.poll.shutdown().await;
}

#[tokio::test]
async fn actual_tail_and_repeat_remain_normal_completions() {
    for mode in [PlaybackMode::Sequential, PlaybackMode::SingleLoop] {
        let (f, before) = Fixture::start(vec![song(1, true)], mode, "t1", []).await;
        f.eof();
        let after = f
            .wait(|view| match mode {
                PlaybackMode::Sequential => view.state == PlaybackStatus::Ended,
                PlaybackMode::SingleLoop => {
                    view.state == PlaybackStatus::Playing && view.play_id != before.play_id
                }
                _ => unreachable!(),
            })
            .await;
        assert_eq!(after.entry_id, before.entry_id);
        if mode == PlaybackMode::Sequential {
            assert_eq!(after.play_id, before.play_id);
            assert_eq!(f.loads(), 1);
            f.assert_durable_end(before.entry_id.as_deref());
        } else {
            assert_eq!(f.loads(), 2);
        }
        f.assert_wire(&["1"]);
        f.poll.shutdown().await;
    }
}

#[tokio::test]
async fn signed_eof_honors_the_original_hundred_denial_stop_limit() {
    let rows = (1..=102).map(|id| song(id, id == 1 || id == 102)).collect();
    let (f, before) =
        Fixture::start(rows, PlaybackMode::Sequential, "t1", [url(2, false).into()]).await;
    f.wire.wait(4).await;
    f.eof();
    let stopped = f.wait(|view| view.state == PlaybackStatus::Idle).await;
    assert_eq!(stopped.entry_id, before.entry_id);
    assert!(stopped.play_id.is_none());
    assert_eq!(f.loads(), 1);
    f.assert_wire(&["1", "2"]);
    f.assert_durable_end(before.entry_id.as_deref());
    f.poll.shutdown().await;
}
