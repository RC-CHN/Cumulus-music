#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback_fixture;

use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken},
    playback::{
        PlaybackMode, Step,
        controller::{
            PlaybackCommand as Cmd, PlaybackController, PlaybackSnapshot, PlaybackStatus,
        },
        engine::Cancellation,
        failure::PlaybackFailure,
        preflight::OrdinaryQueueEnvironment,
        skip_policy::{OrdinarySkipDecision as Decision, SkipBoundary},
        source::SourceResolver,
    },
    protocol::{
        api::{Api, ApiFuture, identifier},
        pc_experiments::PcExperimentGroups,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

const ONLINE: OrdinaryQueueEnvironment =
    OrdinaryQueueEnvironment::OnlineWithoutMvExperiment { app_online: true };

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
}

#[derive(Default)]
struct Wire {
    authorization: Mutex<Vec<u64>>,
    deny: AtomicU64,
    details: Mutex<Vec<Value>>,
    gate: Mutex<Option<Arc<Gate>>>,
}
impl Api for Wire {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            match path {
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(body["ids"].as_str().unwrap()).unwrap();
                    let id = identifier(&ids[0]).unwrap().parse().unwrap();
                    self.authorization.lock().unwrap().push(id);
                    let row = if self.deny.load(Ordering::SeqCst) == id {
                        json!({"id":id,"code":403})
                    } else {
                        json!({"id":id,"code":200,"url":"https://m1.music.126.net/fixture.mp3","level":"standard","md5":"abcd","expi":120})
                    };
                    Ok(json!({"code":200,"data":[row]}))
                }
                "/api/v3/song/detail" => {
                    assert_eq!(
                        body,
                        json!({"c":"[{\"id\":\"90\",\"v\":0}]","trialMode":-1})
                    );
                    self.details.lock().unwrap().push(body);
                    let gate = self.gate.lock().unwrap().clone();
                    if let Some(gate) = gate {
                        gate.entered.notify_one();
                        gate.release.notified().await;
                    }
                    Ok(json!({"code":200,"songs":[{"id":90}],"privileges":[{"id":90,"pl":128000}]}))
                }
                _ => panic!("unexpected transport fixture route {path}"),
            }
        })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    wire: Arc<Wire>,
    session: Arc<AccountSession>,
    player: Arc<PlaybackController>,
    device: Arc<playback_fixture::Device>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
        let wire = Arc::new(Wire::default());
        let session = Arc::new(AccountSession::new(wire.clone(), Some(store.clone())));
        session
            .adopt(wire.clone(), json!({"userId":7}), &session.token().unwrap())
            .unwrap();
        let device = Arc::new(playback_fixture::Device::default());
        let player = Arc::new(
            PlaybackController::start(
                session.clone(),
                store,
                Arc::new(SourceResolver::new(session.clone(), None)),
                playback_fixture::audio_engine(device.clone()),
            )
            .unwrap(),
        );
        Self {
            _root: root,
            wire,
            session,
            player,
            device,
        }
    }

    fn token(&self) -> SessionToken {
        self.session.token().unwrap()
    }

    async fn start(&self, rows: Vec<Value>) -> PlaybackSnapshot {
        let token = self.token();
        self.player
            .command(&token, Cmd::Mode(PlaybackMode::Sequential))
            .await
            .unwrap();
        self.player
            .command(
                &token,
                Cmd::Replace {
                    resources: rows,
                    index: 0,
                    source: json!({"kind":"playlist"}),
                },
            )
            .await
            .unwrap()
    }
}

fn row(id: usize, allowed: bool) -> Value {
    let privilege = if allowed {
        json!({"pl":128000,"st":0,"flag":0,"fee":0})
    } else {
        json!({"pl":0,"st":-200,"flag":0,"fee":0})
    };
    json!({"id":id,"song":{"id":id,"privilege":privilege,"dt":120000}})
}

fn experiment(enabled: bool) -> PcExperimentGroups {
    PcExperimentGroups::refreshed(&json!({"data":[{
        "expName":"PH-PC-skipSongs2","expGroupName":if enabled {"t1"} else {"c"}
    }]}))
    .unwrap()
}

#[tokio::test]
async fn mini_bar_retries_known_denials_then_loads_one_existing_online_row_through_actor() {
    let f = Fixture::new();
    let before = f
        .start(vec![
            row(1, true),
            row(2, false),
            row(3, false),
            row(4, true),
        ])
        .await;
    let token = f.token();
    let original = f.player.queue(&token).await.unwrap();
    let disabled = f
        .player
        .ordinary_minibar_skip(
            &token,
            before.fence(),
            Step::Next,
            &experiment(false),
            ONLINE,
            Cancellation::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        disabled.decision,
        Decision::KeepCurrentDenied {
            attempted: original.items[1].entry_id.clone()
        }
    );
    assert_eq!(disabled.applied.unwrap().fence(), before.fence());
    assert_eq!(f.player.snapshot().play_id, before.play_id);
    let result = f
        .player
        .ordinary_minibar_skip(
            &token,
            before.fence(),
            Step::Next,
            &experiment(true),
            ONLINE,
            Cancellation::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        result.decision,
        Decision::PlayExisting {
            attempted: original.items[1].entry_id.clone(),
            target: original.items[3].entry_id.clone(),
            denial_count: 2,
            at_ordered_tail: false,
        }
    );
    let after = result.applied.unwrap();
    assert_eq!(after.state, PlaybackStatus::Playing);
    assert_ne!(after.play_id, before.play_id);
    assert_eq!(after.selected.unwrap().entry_id, original.items[3].entry_id);
    assert_eq!(f.player.queue(&token).await.unwrap().items, original.items);
    assert_eq!(f.device.state.lock().unwrap().loads, 2);
    assert_eq!(
        f.wire
            .authorization
            .lock()
            .unwrap()
            .iter()
            .filter(|id| **id == 4)
            .count(),
        1
    );
    assert!(f.wire.details.lock().unwrap().is_empty());
}

#[tokio::test]
async fn no_candidate_pauses_existing_play_id_and_does_not_authorize_a_new_row() {
    let f = Fixture::new();
    // An already loaded grant can remain valid while refreshed catalog
    // metadata no longer permits new playback for any of these rows.
    f.start(vec![row(1, false), row(2, false)]).await;
    let token = f.token();
    let before = f.player.command(&token, Cmd::Seek(17.0)).await.unwrap();
    let queue = f.player.queue(&token).await.unwrap();
    let result = f
        .player
        .ordinary_minibar_skip(
            &token,
            before.fence(),
            Step::Next,
            &experiment(true),
            ONLINE,
            Cancellation::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        result.decision,
        Decision::PauseCurrent {
            attempted: queue.items[1].entry_id.clone()
        }
    );
    let after = result.applied.unwrap();
    assert_eq!(after.state, PlaybackStatus::Paused);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.position, before.position);
    assert_eq!(f.player.queue(&token).await.unwrap(), queue);
    assert_eq!(f.device.state.lock().unwrap().loads, 1);
}

#[tokio::test]
async fn transport_403_is_neither_initial_denial_nor_a_reason_to_skip_another_row() {
    for first_denied in [false, true] {
        let f = Fixture::new();
        let target = if first_denied { 3 } else { 2 };
        f.wire.deny.store(target, Ordering::SeqCst);
        let before = f
            .start(vec![
                row(1, true),
                row(2, !first_denied),
                row(3, true),
                row(4, true),
            ])
            .await;
        let token = f.token();
        let result = f
            .player
            .ordinary_minibar_skip(
                &token,
                before.fence(),
                Step::Next,
                &experiment(true),
                ONLINE,
                Cancellation::default(),
            )
            .await;
        if first_denied {
            assert!(result.err().unwrap().is_service_code(403));
        } else {
            assert_eq!(
                result.unwrap().decision,
                Decision::Unchanged(SkipBoundary::InitialRightsNotProven)
            );
            assert_eq!(f.player.snapshot().play_id, before.play_id);
            assert!(
                f.player
                    .command(&token, Cmd::Next)
                    .await
                    .err()
                    .unwrap()
                    .is_service_code(403)
            );
        }
        let after = f.player.snapshot();
        assert_eq!(after.state, PlaybackStatus::Error);
        assert!(matches!(
            after.error,
            Some(PlaybackFailure::SourceUnavailable)
        ));
        assert_eq!(after.selected.unwrap().resource["id"], target);
        assert!(!f.wire.authorization.lock().unwrap().contains(&4));
        assert_eq!(f.device.state.lock().unwrap().loads, 1);
    }
}

#[tokio::test]
async fn slow_preflight_cannot_override_newer_transport_or_account() {
    for switch_account in [false, true] {
        let f = Fixture::new();
        let mut replacement = row(4, false);
        replacement["song"]["noCopyrightRcmd"] = json!({"type":1,"songId":90});
        let before = f
            .start(vec![row(1, true), row(2, false), row(3, true), replacement])
            .await;
        let token = f.token();
        let gate = Arc::new(Gate::default());
        *f.wire.gate.lock().unwrap() = Some(gate.clone());
        let old = {
            let player = f.player.clone();
            let token = token.clone();
            tokio::spawn(async move {
                player
                    .ordinary_minibar_skip(
                        &token,
                        before.fence(),
                        Step::Next,
                        &experiment(true),
                        ONLINE,
                        Cancellation::default(),
                    )
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
            .await
            .unwrap();
        if switch_account {
            f.session
                .adopt(f.wire.clone(), json!({"userId":8}), &token)
                .unwrap();
        } else {
            f.player.command(&token, Cmd::Seek(27.0)).await.unwrap();
        }
        gate.release.notify_one();
        let error = tokio::time::timeout(Duration::from_secs(5), old)
            .await
            .unwrap()
            .unwrap()
            .err()
            .unwrap();
        assert!(matches!(error, Error::StaleResponse));
        assert!(!f.wire.authorization.lock().unwrap().contains(&3));
        assert_eq!(f.device.state.lock().unwrap().loads, 1);
    }
}
