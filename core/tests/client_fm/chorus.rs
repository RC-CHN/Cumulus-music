use super::{domain, engine};
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{FmCommand, PlayerAction, PlayerView},
    },
    fm::{FmRights, FmService, FmSource},
    membership::Quality,
    protocol::{
        api::{Api, ApiFuture, PlaybackChorusPermission, PlaybackChorusRead},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

struct Reply {
    value: Result<Value>,
    held: bool,
}

struct Wire {
    fm: domain::Backend,
    credential: AtomicU64,
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<Value>>,
    entered: Notify,
    released: Notify,
    verified: Notify,
}

impl Wire {
    fn response(&self, points: &[(u64, u64)], held: bool) {
        let rows: Vec<_> = points
            .iter()
            .map(|(id, seconds)| json!({"id":id,"startTime":seconds*1000}))
            .collect();
        self.replies.lock().unwrap().push_back(Reply {
            value: Ok(json!({"code":200,"data":rows})),
            held,
        });
    }

    fn failure(&self) {
        self.replies.lock().unwrap().push_back(Reply {
            value: Err(Error::Transport(
                "synthetic optional chorus read failed".into(),
            )),
            held: false,
        });
    }

    fn ids(&self) -> Vec<Vec<u64>> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|payload| {
                assert_eq!(payload.as_object().unwrap().len(), 1, "M15 sends only ids");
                serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap()
            })
            .collect()
    }
}

impl Api for Wire {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cookie: bool) -> ApiFuture<'a, Value> {
        assert_ne!(
            path, "/api/song/chorus",
            "FM marker must not fall through to a singleton"
        );
        self.fm.request(path, payload, cookie)
    }

    fn observed_playback_chorus_revision(&self) -> Option<String> {
        Some(self.credential.load(Ordering::SeqCst).to_string())
    }

    fn playback_chorus_revision(&self) -> Result<Option<String>> {
        self.verified.notify_one();
        Ok(self.observed_playback_chorus_revision())
    }

    fn request_playback_chorus<'a>(
        &'a self,
        read: PlaybackChorusRead,
        revision: &'a str,
        permission: &'a PlaybackChorusPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            permission()?;
            assert_eq!(
                self.observed_playback_chorus_revision().as_deref(),
                Some(revision)
            );
            let (path, payload) = read.into_request()?;
            assert_eq!(path, "/api/song/chorus");
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected FM chorus dispatch");
            self.calls.lock().unwrap().push(payload);
            self.entered.notify_one();
            if reply.held {
                self.released.notified().await;
            }
            reply.value
        })
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    session: Arc<AccountSession>,
    wire: Arc<Wire>,
    client: Arc<MusicClient>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
        let wire = Arc::new(Wire {
            fm: Default::default(),
            credential: AtomicU64::new(1),
            replies: Default::default(),
            calls: Default::default(),
            entered: Notify::new(),
            released: Notify::new(),
            verified: Notify::new(),
        });
        let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
        session
            .adopt(wire.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let client = MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
        Self {
            _dir: dir,
            session,
            wire,
            client: Arc::new(client),
        }
    }

    async fn start(&self, source: FmSource) -> PlayerView {
        self.client
            .fm_command(
                "1",
                source,
                FmCommand::Start {
                    quality: Quality::Standard,
                },
            )
            .await
            .unwrap();
        let player = self.client.player_snapshot("1").await.unwrap();
        assert!(player.fm && player.timeline.is_some());
        player
    }

    async fn mark(&self, player: &PlayerView) -> Result<Option<f64>> {
        self.client
            .player_chorus("1", player.play_id.as_deref().unwrap())
            .await
    }

    async fn pending_mark(
        &self,
        player: &PlayerView,
    ) -> tokio::task::JoinHandle<Result<Option<f64>>> {
        // The transport's request is already held. Consume its existing
        // verification signal, then wait until the marker checks that same
        // native receipt before it joins the pending request.
        event(&self.wire.verified).await;
        let client = self.client.clone();
        let play = player.play_id.clone().unwrap();
        let marker = tokio::spawn(async move { client.player_chorus("1", &play).await });
        event(&self.wire.verified).await;
        assert!(!marker.is_finished());
        marker
    }
}

async fn event(event: &Notify) {
    tokio::time::timeout(Duration::from_secs(2), event.notified())
        .await
        .expect("sealed FM observation");
}

#[tokio::test]
async fn fm_load_more_shares_personal_points_and_merges_only_after_actual_refill() {
    for refill in [None, Some(true), Some(false)] {
        let f = Fixture::new();
        if refill.is_some() {
            f.wire.fm.batch(&[1, 2]);
            f.wire.fm.batch(&[2]);
        } else {
            f.wire.fm.batch(&[1, 2, 3, 4]);
        }
        f.wire.response(&[(1, 2), (2, 3), (99, 20)], false);
        // Actual page preload is enough. No playback marker has requested a read.
        f.client
            .fm_view("1", FmSource::Personal, true)
            .await
            .unwrap();
        event(&f.wire.entered).await;
        let first = f.start(FmSource::Personal).await;
        assert_eq!(f.mark(&first).await.unwrap(), Some(2.0));
        if let Some(success) = refill {
            f.wire.fm.batch(&[3, 4, 5]);
            if success {
                // Id 2 is omitted: retain its previous point, as M1384 merges.
                f.wire.response(&[(3, 7), (99, 20)], false);
            } else {
                f.wire.failure();
            }
        }
        let next = f
            .client
            .player_action("1", PlayerAction::Next)
            .await
            .unwrap();
        assert_ne!(next.play_id, first.play_id);
        assert_eq!(next.track.as_ref().unwrap().id, "2");
        assert_eq!(f.mark(&next).await.unwrap(), Some(3.0));
        let expected = if refill.is_some() {
            vec![vec![1, 2], vec![1, 2, 3, 4, 5]]
        } else {
            vec![vec![1, 2, 3, 4]]
        };
        assert_eq!(f.wire.ids(), expected);
    }
}

#[tokio::test]
async fn fm_personal_familiar_and_zone_keep_overlapping_song_points_separate() {
    let f = Fixture::new();
    f.wire.fm.batch(&[1, 2, 3]);
    f.wire.response(&[(1, 2)], true);
    let first = f.start(FmSource::Personal).await;
    event(&f.wire.entered).await;
    let old = f.pending_mark(&first).await;

    let familiar = FmSource::Familiar {
        source_id: "[\"11\",\"12\"]".into(),
    };
    f.wire.fm.batch(&[1, 4, 5]);
    f.wire.response(&[(1, 6)], false);
    let second = f.start(familiar.clone()).await;
    assert_eq!(f.mark(&second).await.unwrap(), Some(6.0));

    let zone = FmSource::Zone {
        rule_id: "70".into(),
        trial_mode: Some(121),
    };
    f.wire.fm.batch(&[1, 8, 9]);
    f.wire.fm.responses.lock().unwrap().insert(
        "/api/v3/song/detail".into(),
        json!({"code":200,"songs":[domain::song(1),domain::song(8),domain::song(9)]}),
    );
    f.wire.response(&[(1, 8)], false);
    let third = f.start(zone).await;
    assert_eq!(f.mark(&third).await.unwrap(), Some(8.0));
    f.wire.released.notify_one();
    assert!(matches!(old.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(f.mark(&third).await.unwrap(), Some(8.0));
    let familiar_again = f.start(familiar).await;
    assert_eq!(f.mark(&familiar_again).await.unwrap(), Some(6.0));
    assert_eq!(f.wire.ids(), [vec![1, 2, 3], vec![1, 4, 5], vec![1, 8, 9]]);
    let calls = f.wire.fm.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|(path, args)| path == "/api/v1/radio/get" && args["mode"] == "FAMILIAR")
    );
    assert!(
        calls
            .iter()
            .any(|(path, args)| path == "/api/song/enhance/player/url/v1"
                && args["trialMode"] == 121)
    );
}

#[tokio::test]
async fn fm_held_load_is_fenced_by_new_source_revision_cookie_or_account() {
    enum Change {
        Refill,
        Cookie,
        Owner,
        Api,
    }
    for change in [Change::Refill, Change::Cookie, Change::Owner, Change::Api] {
        let f = Fixture::new();
        f.wire.fm.batch(&[1, 2, 3]);
        f.wire.response(&[(1, 2)], true);
        let current = f.start(FmSource::Personal).await;
        event(&f.wire.entered).await;
        let old = f.pending_mark(&current).await;
        match change {
            Change::Refill => {
                // A second native coordinator updates the same durable lane.
                // The old in-process flight is still held and must check SQLite.
                let other = FmService::new(f.session.clone()).unwrap();
                f.wire.fm.batch(&[4, 5, 6]);
                f.wire.response(&[(1, 6)], false);
                other
                    .preload_for(
                        &f.session.token().unwrap(),
                        FmSource::Personal,
                        FmRights {
                            vinyl: true,
                            plus: false,
                        },
                    )
                    .await
                    .unwrap();
                event(&f.wire.entered).await;
            }
            Change::Cookie => {
                f.wire.credential.store(2, Ordering::SeqCst);
            }
            Change::Owner => {
                f.session
                    .adopt(
                        f.wire.clone(),
                        json!({"userId":10}),
                        &f.session.token().unwrap(),
                    )
                    .unwrap();
            }
            Change::Api => {
                f.session
                    .adopt(
                        Arc::new(domain::Backend::default()),
                        json!({"userId":9}),
                        &f.session.token().unwrap(),
                    )
                    .unwrap();
            }
        }
        f.wire.released.notify_one();
        assert!(matches!(old.await.unwrap(), Err(Error::StaleResponse)));
        assert_eq!(
            f.wire.ids().len(),
            if matches!(change, Change::Refill) {
                2
            } else {
                1
            }
        );
    }
}

#[tokio::test]
async fn fm_uses_one_ordered_bucket_split_at_the_official_500_ids() {
    let f = Fixture::new();
    let ids: Vec<_> = (1..=501).collect();
    f.wire.fm.batch(&ids);
    f.wire.response(&[(1, 2)], false);
    f.wire.response(&[(501, 7)], false);
    let first = f.start(FmSource::Personal).await;
    assert_eq!(f.mark(&first).await.unwrap(), Some(2.0));
    assert_eq!(f.wire.ids(), [ids[..500].to_vec(), vec![501]]);
}

#[tokio::test]
async fn fm_does_not_authorize_chorus_with_a_cookie_replaced_during_recommendations() {
    let f = Fixture::new();
    f.wire.fm.batch(&[1, 2, 3]);
    *f.wire.fm.hold.lock().unwrap() = Some("/api/v1/radio/get".into());
    let preload = tokio::spawn({
        let client = f.client.clone();
        async move { client.fm_view("1", FmSource::Personal, true).await }
    });
    event(&f.wire.fm.started).await;
    f.wire.credential.store(2, Ordering::SeqCst);
    f.wire.fm.release.notify_one();
    let view = preload.await.unwrap().unwrap();
    assert_eq!(
        view.value.items.len(),
        3,
        "decoration does not block the real source"
    );
    let active = f.start(FmSource::Personal).await;
    assert_eq!(f.mark(&active).await.unwrap(), None);
    assert!(f.wire.ids().is_empty());
}
