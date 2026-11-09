use super::{
    ASSIGNMENT_PATH, playback,
    support::http::{HttpResponse, ScriptHttp, gate, reply},
    vinyl_http,
};
use cumulus_core::{
    account::AccountSession,
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
    protocol::api::identifier,
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
        Self::start_at(rows, mode, group, 0, None, replies).await
    }

    async fn start_at(
        rows: Vec<Value>,
        mode: PlaybackMode,
        group: &str,
        current: usize,
        order: Option<&[usize]>,
        replies: impl IntoIterator<Item = HttpResponse>,
    ) -> (Self, PlayerView) {
        let initial = rows[current]["id"].as_u64().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (api, wire) = vinyl_http(
            root.path(),
            &["MUSIC_U=synthetic"],
            [
                json!({"code":200,"data":[{"expName":"PH-PC-skipSongs2","expGroupName":group}]})
                    .into(),
                json!({"code":200,"msg":0,"comment":0,"forward":0,"notice":0}).into(),
                url(initial, true).into(),
            ]
            .into_iter()
            .chain(replies),
        );
        let registry = super::fixture_registry(&root.path().join("host"));
        let store = registry.store();
        let mut queue = PlaybackQueue::new(Some(store.clone()), "71").unwrap();
        queue.set_mode(mode).unwrap();
        queue
            .replace(rows, current, json!({"kind":"playlist"}))
            .unwrap();
        if let Some(order) = order {
            // Use an explicit persisted native deck, not a lucky RNG draw.
            assert_eq!(mode, PlaybackMode::Random);
            let mut snapshot = queue.snapshot();
            snapshot.order = order
                .iter()
                .map(|&i| snapshot.items[i].entry_id.clone())
                .collect();
            store
                .put(
                    "71",
                    "playback",
                    "queue",
                    &serde_json::to_value(snapshot).unwrap(),
                )
                .unwrap();
        }
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
        let mut notices = f.client.subscribe_ordinary_skip(&f.epoch).unwrap();
        f.eof();
        let after = f.wait(|view| view.state == expected).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(60), notices.next())
                .await
                .is_err()
        );
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
    let mut notices = f.client.subscribe_ordinary_skip(&f.epoch).unwrap();
    f.eof();
    f.wire.wait(5).await;
    // URL 3 is still blocked; the already committed selection owns the notice.
    let notice = tokio::time::timeout(Duration::from_secs(2), notices.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notice.epoch, f.epoch);
    assert_eq!(notice.id.len(), 32);
    assert!((1..=3000).contains(&notice.remaining_ms));
    let mut late = f.client.subscribe_ordinary_skip(&f.epoch).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(60), late.next())
            .await
            .is_err()
    );
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    let _ = release.send(Ok(reply(url(3, true))));
    let after = f.view().await;
    assert_eq!(after.state, PlaybackStatus::Idle);
    assert!(after.play_id.is_none());
    assert_eq!(f.loads(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(60), notices.next())
            .await
            .is_err()
    );
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

#[tokio::test]
async fn signed_loop_eof_follows_display_or_persisted_random_order_when_wrapping() {
    for (mode, rows, order, denied, target) in [
        (
            PlaybackMode::OrderLoop,
            vec![song(1, false), song(2, true), song(3, true)],
            None,
            1,
            2,
        ),
        (
            PlaybackMode::Random,
            vec![song(1, true), song(2, false), song(3, true)],
            Some(&[1, 0, 2][..]),
            2,
            1,
        ),
    ] {
        let (f, before) = Fixture::start_at(
            rows,
            mode,
            "t1",
            2,
            order,
            [url(denied, false).into(), url(target, true).into()],
        )
        .await;
        let denied = denied.to_string();
        let target = target.to_string();
        f.wire.wait(4).await;
        f.assert_wire(&["3", &denied]);
        let original = PlaybackQueue::new(Some(f.store.clone()), "71")
            .unwrap()
            .snapshot();
        f.eof();
        let after = f
            .wait(|view| view.state == PlaybackStatus::Playing && view.play_id != before.play_id)
            .await;
        assert_eq!(after.track.unwrap().id, target);
        assert_eq!(after.mode, mode);
        assert_eq!(f.loads(), 2);
        let actual = PlaybackQueue::new(Some(f.store.clone()), "71")
            .unwrap()
            .snapshot();
        assert_eq!(actual.order, original.order);
        assert_eq!(actual.items, original.items);
        assert_eq!(actual.current, after.entry_id);
        assert_eq!(
            actual
                .items
                .iter()
                .find(|row| Some(&row.entry_id) == actual.current.as_ref())
                .unwrap()
                .resource["id"]
                .as_u64(),
            target.parse::<u64>().ok()
        );
        f.assert_wire(&["3", &denied, &target]);
        f.poll.shutdown().await;
    }
}

fn ended_step(view: &PlayerView, direction: cumulus_core::playback::Step) -> PlayerAction {
    PlayerAction::StepEndedMiniBar {
        receipt: view
            .ended_minibar_receipt
            .clone()
            .expect("retained native EOF receipt"),
        entry_id: view.entry_id.clone().unwrap(),
        expected_queue_revision: view.queue_revision.clone(),
        direction,
    }
}

#[tokio::test]
async fn signed_loop_control_clears_only_the_finished_identity_after_a_denied_wrap() {
    use cumulus_core::{Error, playback::Step};

    for (mode, switched_mode) in [
        (PlaybackMode::OrderLoop, PlaybackMode::SingleLoop),
        (PlaybackMode::Random, PlaybackMode::Sequential),
    ] {
        let (f, before) = Fixture::start_at(
            vec![song(1, false), song(2, true), song(3, true)],
            mode,
            "c",
            2,
            (mode == PlaybackMode::Random).then_some(&[0, 1, 2][..]),
            [url(1, false).into()],
        )
        .await;
        f.wire.wait(4).await;
        let stops = f.device.state.lock().unwrap().stops;
        f.eof();
        let after = f
            .wait(|view| view.state == PlaybackStatus::Ended && view.play_id.is_none())
            .await;
        assert_eq!(after.track.as_ref().unwrap().id, "3");
        assert_eq!(after.entry_id, before.entry_id);
        assert_eq!(after.queue_revision, before.queue_revision);
        assert_eq!(after.position, Some(120.0));
        assert_eq!(after.timeline.as_ref().unwrap().total, Some(120.0));
        let action = ended_step(&after, Step::Next);
        let mut notices = f.client.subscribe_ordinary_skip(&f.epoch).unwrap();
        let pauses = f.device.state.lock().unwrap().pauses.len();
        let clicked = f.client.player_action(&f.epoch, action).await.unwrap();
        assert_eq!(clicked.state, PlaybackStatus::Ended);
        assert!(clicked.play_id.is_none());
        assert_eq!(clicked.ended_minibar_receipt, after.ended_minibar_receipt);
        assert_eq!(clicked.entry_id, before.entry_id);
        assert_eq!(clicked.queue_revision, before.queue_revision);
        assert_eq!(clicked.position, Some(120.0));
        assert_eq!(f.device.state.lock().unwrap().stops, stops);
        assert_eq!(f.device.state.lock().unwrap().pauses.len(), pauses);
        let switched = f
            .client
            .player_action(
                &f.epoch,
                PlayerAction::ModeFor {
                    mode: switched_mode,
                    expected_queue_revision: after.queue_revision.clone(),
                    fm: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(switched.mode, switched_mode);
        assert_eq!(switched.state, PlaybackStatus::Ended);
        assert_eq!(switched.entry_id, after.entry_id);
        assert!(switched.play_id.is_none());
        assert_eq!(switched.ended_minibar_receipt, after.ended_minibar_receipt);
        assert_ne!(switched.queue_revision, after.queue_revision);
        assert!(matches!(
            f.client.player_action(&f.epoch, ended_step(&after, Step::Next)).await,
            Err(error) if matches!(error.root(), Error::StaleResponse)
        ));
        let clicked = f
            .client
            .player_action(&f.epoch, ended_step(&switched, Step::Next))
            .await
            .unwrap();
        assert_eq!(clicked.state, PlaybackStatus::Ended);
        assert_eq!(clicked.entry_id, switched.entry_id);
        assert_eq!(clicked.play_id, switched.play_id);
        assert_eq!(clicked.position, Some(120.0));
        assert_eq!(clicked.queue_revision, switched.queue_revision);
        assert_eq!(
            clicked.ended_minibar_receipt,
            switched.ended_minibar_receipt
        );
        assert_eq!(f.device.state.lock().unwrap().stops, stops);
        assert_eq!(f.device.state.lock().unwrap().pauses.len(), pauses);

        // Detaching the actual browser lease removes current signed-read
        // admission for this receipt; there is no legacy step on expiry.
        f.poll.try_browser_lifecycle(None).unwrap();
        assert!(matches!(
            f.client.player_action(&f.epoch, ended_step(&switched, Step::Next)).await,
            Err(error) if matches!(error.root(), Error::StaleResponse)
        ));
        assert_eq!(f.view().await.state, PlaybackStatus::Ended);
        assert_eq!(f.loads(), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(60), notices.next())
                .await
                .is_err()
        );
        f.assert_wire(&["3", "1"]);
        f.assert_durable_end(before.entry_id.as_deref());
        f.poll.shutdown().await;
    }
}

#[tokio::test]
async fn single_loop_manual_next_uses_new_display_order_after_retained_random_eof() {
    use cumulus_core::playback::Step;

    let (f, _) = Fixture::start_at(
        vec![song(1, false), song(3, true), song(2, true)],
        PlaybackMode::Random,
        "c",
        1,
        Some(&[1, 0, 2]),
        [url(1, false).into(), url(2, true).into()],
    )
    .await;
    // Display is [1,3,2], random transport is [3,1,2].
    // The natural attempt is denied song 1; switching to SingleLoop uses the
    // new displayed neighbor 2 for the explicit Next, not self-repeat 3.
    f.wire.wait(4).await;
    f.eof();
    let finished = f.wait(|view| view.ended_minibar_receipt.is_some()).await;
    let switched = f
        .client
        .player_action(
            &f.epoch,
            PlayerAction::ModeFor {
                mode: PlaybackMode::SingleLoop,
                expected_queue_revision: finished.queue_revision.clone(),
                fm: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(switched.mode, PlaybackMode::SingleLoop);
    assert_eq!(
        switched.ended_minibar_receipt,
        finished.ended_minibar_receipt
    );
    assert_ne!(switched.queue_revision, finished.queue_revision);
    let mut notices = f.client.subscribe_ordinary_skip(&f.epoch).unwrap();
    let played = f
        .client
        .player_action(&f.epoch, ended_step(&switched, Step::Next))
        .await
        .unwrap();
    assert_eq!(played.state, PlaybackStatus::Playing);
    assert_eq!(played.mode, PlaybackMode::SingleLoop);
    assert_eq!(played.track.unwrap().id, "2");
    assert!(played.play_id.is_some() && played.ended_minibar_receipt.is_none());
    assert_eq!(f.loads(), 2);
    assert!(
        tokio::time::timeout(Duration::from_millis(60), notices.next())
            .await
            .is_err()
    );
    f.assert_wire(&["3", "1", "2"]);
    f.poll.shutdown().await;
}

#[tokio::test]
async fn ended_bottom_bar_requires_the_current_receipt_and_rechecks_signed_treatment() {
    use cumulus_core::{Error, playback::Step};

    let (f, before) = Fixture::start_at(
        vec![song(1, false), song(2, true), song(3, true)],
        PlaybackMode::Random,
        "c",
        2,
        Some(&[0, 1, 2]),
        [
            url(1, false).into(),
            json!({"code":200,"data":[{"expName":"PH-PC-skipSongs2","expGroupName":"t1"}]}).into(),
            url(2, true).into(),
        ],
    )
    .await;
    assert!(before.ended_minibar_receipt.is_none());
    f.wire.wait(4).await;
    f.eof();
    let finished = f.wait(|view| view.ended_minibar_receipt.is_some()).await;
    let wrong = |receipt: String, entry_id: String, expected_queue_revision: String| {
        PlayerAction::StepEndedMiniBar {
            receipt,
            entry_id,
            expected_queue_revision,
            direction: Step::Next,
        }
    };
    let receipt = finished.ended_minibar_receipt.clone().unwrap();
    for action in [
        wrong(
            "foreign".into(),
            finished.entry_id.clone().unwrap(),
            finished.queue_revision.clone(),
        ),
        wrong(receipt, "foreign".into(), finished.queue_revision.clone()),
    ] {
        assert!(matches!(
            f.client.player_action(&f.epoch, action).await,
            Err(error) if matches!(error.root(), Error::StaleResponse)
        ));
    }
    // A real durable queue write supersedes the version rendered by the old
    // footer, even though the original completed row still exists.
    PlaybackQueue::new(Some(f.store.clone()), "71")
        .unwrap()
        .append(vec![song(4, false)], json!({"kind":"playlist"}))
        .unwrap();
    assert!(matches!(
        f.client.player_action(&f.epoch, ended_step(&finished, Step::Next)).await,
        Err(error) if matches!(error.root(), Error::StaleResponse)
    ));
    let another_epoch = (f.epoch.parse::<u64>().unwrap() + 1).to_string();
    assert!(
        f.client
            .player_action(&another_epoch, ended_step(&finished, Step::Next))
            .await
            .is_err()
    );
    assert_eq!(f.loads(), 1);
    f.assert_wire(&["3", "1"]);

    let current = f.view().await;
    assert_ne!(current.queue_revision, finished.queue_revision);
    assert_eq!(current.entry_id, finished.entry_id);
    // This actual signed full-batch refresh changes native assignment without
    // a new selection, EOF or playback ID for the already-finished row.
    f.client.refresh_pc_presentation().await.unwrap();
    let mut notices = f.client.subscribe_ordinary_skip(&f.epoch).unwrap();
    let played = f
        .client
        .player_action(&f.epoch, ended_step(&current, Step::Next))
        .await
        .unwrap();
    assert_eq!(played.state, PlaybackStatus::Playing);
    assert_eq!(played.track.unwrap().id, "2");
    assert!(played.play_id.is_some());
    assert!(played.ended_minibar_receipt.is_none());
    assert_ne!(played.entry_id, finished.entry_id);
    assert_eq!(f.loads(), 2);
    assert!(matches!(
        f.client.player_action(&f.epoch, ended_step(&current, Step::Previous)).await,
        Err(error) if matches!(error.root(), Error::StaleResponse)
    ));
    let notice = tokio::time::timeout(Duration::from_secs(2), notices.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notice.epoch, f.epoch);
    assert!(
        tokio::time::timeout(Duration::from_millis(60), notices.next())
            .await
            .is_err()
    );
    let calls = f.wire.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            ASSIGNMENT_PATH,
            "/api/pl/count",
            AUTH,
            AUTH,
            ASSIGNMENT_PATH,
            AUTH,
        ]
    );
    for (call, expected) in [&calls[2], &calls[3], &calls[5]]
        .into_iter()
        .zip(["3", "1", "2"])
    {
        let ids: Vec<Value> = serde_json::from_str(call.payload["ids"].as_str().unwrap()).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(identifier(&ids[0]).unwrap(), expected);
    }
    f.poll.shutdown().await;
}

#[tokio::test]
async fn signed_loop_honors_the_denied_attempt_cap_without_leaving_the_old_row() {
    let rows = (1..=102).map(|id| song(id, id == 1 || id == 102)).collect();
    let (f, before) =
        Fixture::start(rows, PlaybackMode::OrderLoop, "t1", [url(2, false).into()]).await;
    f.wire.wait(4).await;
    f.eof();
    let after = f.wait(|view| view.state == PlaybackStatus::Idle).await;
    assert_eq!(after.entry_id, before.entry_id);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert!(after.play_id.is_none());
    assert_eq!(f.loads(), 1);
    f.assert_wire(&["1", "2"]);
    f.assert_durable_end(before.entry_id.as_deref());
    f.poll.shutdown().await;
}
