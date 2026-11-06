use super::*;
use cumulus_core::protocol::pc_experiments::PcExperimentGroups;
use std::time::Duration;

#[derive(Default)]
pub(super) struct CoverFixture {
    mode: AtomicU64,
    revision: AtomicU64,
    response: AtomicU64,
    held: AtomicU64,
    started: Notify,
    release: Notify,
    calls: Mutex<Vec<&'static str>>,
}

impl CoverFixture {
    pub(super) fn is_enabled(&self) -> bool {
        self.mode.load(Ordering::SeqCst) != 0
    }

    pub(super) fn revision(&self) -> Option<String> {
        (self.mode.load(Ordering::SeqCst) != 4)
            .then(|| self.revision.load(Ordering::SeqCst).to_string())
    }

    pub(super) fn groups(&self, revision: &str) -> cumulus_core::Result<PcExperimentGroups> {
        self.calls.lock().unwrap().push("experiment");
        assert_eq!(self.revision().as_deref(), Some(revision));
        let assignment = match self.mode.load(Ordering::SeqCst) {
            1 => json!("t1"),
            2 => json!("c"),
            3 => json!("t1 "),
            5 => return PcExperimentGroups::refreshed(&json!({"data":null})),
            6 => {
                return PcExperimentGroups::refreshed(&json!({
                    "data":[{"expName":"unrelated","expGroupName":"t1"}]
                }));
            }
            n => panic!("unexpected experiment for {n}"),
        };
        PcExperimentGroups::refreshed(&json!({
            "data":[{"expName":"PH-PC-Singer-Top50","expGroupName":assignment}]
        }))
    }

    pub(super) async fn request(
        &self,
        path: &str,
        payload: &Value,
        cache: bool,
    ) -> Option<cumulus_core::Result<Value>> {
        if !self.is_enabled() {
            return None;
        }
        let track = |id, privilege| {
            json!({"id":id,"name":format!("歌手曲目 {id}"),"duration":120000,
                "artists":[{"id":9123,"name":"测试歌手"}],
                "album":{"id":7,"name":"测试专辑"},"privilege":privilege,
                "url":"https://private.invalid/?token=SECRET","cookie":"SECRET"})
        };
        Some(Ok(match path {
            "/api/v2/artist/songs" => {
                self.calls.lock().unwrap().push("regular");
                assert_eq!(payload, &json!({"id":"9123","offset":0,"limit":50}));
                assert!(!cache);
                json!({"code":200,"songs":[track(83,json!({"pl":320000})),
                    track(81,json!({"pl":0,"flag":1024})),track(82,json!({"pl":0}))]})
            }
            "/api/artist/v3/detail" => {
                self.calls.lock().unwrap().push("detail");
                assert_eq!(payload, &json!({"id":"9123","top":50,"e_r":false}));
                assert!(cache);
                if self.held.swap(0, Ordering::SeqCst) == 1 {
                    self.started.notify_one();
                    self.release.notified().await;
                }
                let blocked = track(73, json!({"pl":128000}));
                let repeated = track(72, json!({"pl":0}));
                let songs = match self.response.load(Ordering::SeqCst) {
                    1 => vec![],
                    2 => return Some(Err(Error::Transport("synthetic detail failure".into()))),
                    _ => vec![blocked, repeated.clone(), repeated, track(71, Value::Null)],
                };
                json!({"code":200,"artist":{"id":9123,"name":"已核对的歌手来源"},"hotSongs":songs})
            }
            "/api/song/enhance/privilege" => {
                self.calls.lock().unwrap().push("privileges");
                assert!(!cache);
                match self.response.load(Ordering::SeqCst) {
                    1 => {
                        assert_eq!(payload, &json!({"ids":"[]"}));
                        json!({"code":200,"data":[]})
                    }
                    mode => {
                        assert_eq!(payload, &json!({"ids":"[\"73\",\"72\",\"72\",\"71\"]"}));
                        if mode == 3 {
                            return Some(Err(Error::Transport(
                                "synthetic privileges failure".into(),
                            )));
                        }
                        json!({"code":200,"data":[{"id":71,"pl":320000},{"id":72,"pl":128000},
                            {"id":73,"pl":0}]})
                    }
                }
            }
            _ => return None,
        }))
    }
}

fn artist() -> PlaySelection {
    PlaySelection::SearchArtistCard {
        id: "9123".into(),
        quality: Quality::Lossless,
    }
}

fn ticket(client: &MusicClient, view: &PlayerView) -> String {
    client
        .prepare_play_selection(&view.epoch, &view.queue_revision, view.play_id.as_deref())
        .unwrap()
}

#[tokio::test]
async fn signed_artist_cover_uses_exact_lane_and_server_order_without_detail_refetch() {
    for mode in 1..=6 {
        let (_dir, api, _session, client, _count) = fixture();
        api.artist_cover.mode.store(mode, Ordering::SeqCst);
        let idle = client.player_snapshot("1").await.unwrap();
        let view = client
            .commit_prepared_selection("1", &ticket(&client, &idle), artist())
            .await
            .unwrap();
        let queue = serde_json::to_value(client.player_queue("1").await.unwrap()).unwrap();
        let exposed = serde_json::to_string(&view).unwrap() + &queue.to_string();
        assert!(!exposed.contains("SECRET"));
        assert!(matches!(view.origin, Some(PlayerOrigin::Artist { id }) if id == "9123"));
        assert_eq!(
            view.origin_name.as_deref(),
            (mode == 1).then_some("已核对的歌手来源")
        );
        let (expected, paths) = match mode {
            1 => (["72", "71"], &["experiment", "detail", "privileges"][..]),
            4 => (["83", "81"], &["regular"][..]),
            _ => (["83", "81"], &["experiment", "regular"][..]),
        };
        assert_eq!(view.track.unwrap().id, expected[0]);
        assert_eq!(queue["tracks"][0]["track"]["id"], expected[0]);
        assert_eq!(queue["tracks"][1]["track"]["id"], expected[1]);
        assert_eq!(queue["tracks"].as_array().unwrap().len(), 2);
        assert_eq!(*api.artist_cover.calls.lock().unwrap(), paths);
        assert!(
            !api.calls
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.contains("song/detail"))
        );
    }
}

#[tokio::test]
async fn failed_or_empty_hot_list_never_fetches_control_or_replaces_current_queue() {
    for (mode, calls) in [
        (1, &["experiment", "detail", "privileges"][..]),
        (2, &["experiment", "detail"][..]),
        (3, &["experiment", "detail", "privileges"][..]),
    ] {
        let (_dir, api, _session, client, _count) = fixture();
        client.play_selection("1", songs("1")).await.unwrap();
        let paused = client
            .player_action("1", PlayerAction::Pause { paused: true })
            .await
            .unwrap();
        api.artist_cover.mode.store(1, Ordering::SeqCst);
        api.artist_cover.response.store(mode, Ordering::SeqCst);
        assert!(
            client
                .commit_prepared_selection("1", &ticket(&client, &paused), artist())
                .await
                .is_err()
        );
        let current = client.player_snapshot("1").await.unwrap();
        assert_eq!(current.play_id, paused.play_id);
        assert_eq!(current.state, paused.state);
        assert_eq!(current.queue_revision, paused.queue_revision);
        assert_eq!(*api.artist_cover.calls.lock().unwrap(), calls);
    }
}

#[tokio::test]
async fn retired_credentials_and_account_cannot_apply_artist_cover_or_start_privilege_read() {
    for switch_account in [false, true] {
        let (_dir, api, session, client, _count) = fixture();
        let old = client.play_selection("1", songs("1")).await.unwrap();
        api.artist_cover.mode.store(1, Ordering::SeqCst);
        api.artist_cover.held.store(1, Ordering::SeqCst);
        let ticket = ticket(&client, &old);
        let pending = {
            let client = client.clone();
            tokio::spawn(async move {
                client
                    .commit_prepared_selection("1", &ticket, artist())
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(4), api.artist_cover.started.notified())
            .await
            .unwrap();
        if switch_account {
            session
                .adopt(
                    Arc::new(Backend::default()),
                    json!({"userId":10}),
                    &session.token().unwrap(),
                )
                .unwrap();
        } else {
            api.artist_cover.revision.store(2, Ordering::SeqCst);
        }
        api.artist_cover.release.notify_one();
        assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
        assert_eq!(
            *api.artist_cover.calls.lock().unwrap(),
            ["experiment", "detail"]
        );
        if switch_account {
            let now = client.player_snapshot("2").await.unwrap();
            assert_eq!(now.queue_len, 0);
            assert!(now.track.is_none());
        } else {
            let now = client.player_snapshot("1").await.unwrap();
            assert_eq!(now.play_id, old.play_id);
            assert_eq!(now.queue_revision, old.queue_revision);
        }
    }
}
