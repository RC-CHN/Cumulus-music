use super::{fixture, songs};
use cumulus_core::{
    Error, Result,
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction, PlayerView, QueueEdit},
    },
    membership::Quality,
    playback::{PlaybackQueue, controller::PlaybackStatus},
    protocol::api::{PlaybackChorusPermission, PlaybackChorusRead},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::{path::Path, time::Duration};
use tokio::{sync::Notify, time::Instant};

#[derive(Default)]
pub(super) struct Backend {
    credentials: AtomicU64,
    hold_first: AtomicBool,
    started: Notify,
    second_started: Notify,
    release: Notify,
    calls: Mutex<Vec<Vec<String>>>,
    called: Notify,
    dispatch_at: Mutex<Vec<Instant>>,
    singles: Mutex<Vec<String>>,
}

impl Backend {
    pub(super) fn revision(&self) -> Option<String> {
        let credential = self.credentials.load(Ordering::SeqCst);
        (credential != 0).then(|| credential.to_string())
    }

    fn ids(payload: &Value) -> Vec<String> {
        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap()
    }

    pub(super) fn single(&self, payload: &Value) -> Value {
        let ids = Self::ids(payload);
        assert_eq!(ids.len(), 1);
        self.singles.lock().unwrap().extend(ids);
        json!({"code":200,"data":[{"id":1,"startTime":9000}]})
    }

    pub(super) async fn batch(
        &self,
        read: PlaybackChorusRead,
        revision: &str,
        permission: &PlaybackChorusPermission<'_>,
    ) -> Result<Value> {
        permission()?;
        assert_eq!(self.revision().as_deref(), Some(revision));
        let (path, payload) = read.into_request()?;
        assert_eq!(path, "/api/song/chorus");
        let ids = Self::ids(&payload);
        {
            let mut calls = self.calls.lock().unwrap();
            calls.push(ids.clone());
            self.dispatch_at.lock().unwrap().push(Instant::now());
            self.called.notify_one();
            if calls.len() == 2 {
                self.second_started.notify_one();
            }
        }
        if self.hold_first.swap(false, Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
        let delta = revision.parse::<u64>().unwrap();
        let mut rows = ids
            .iter()
            .filter(|id| *id != "3")
            .map(|id| json!({"id":id,"startTime":1000*(id.parse::<u64>().unwrap()+delta)}))
            .collect::<Vec<_>>();
        rows.push(json!({"id":"3","startTime":30000})); // unsolicited on [1,2]
        if ids.contains(&"3".to_string()) {
            rows.push(json!({"id":"3","startTime":0})); // later invalid row wins
        }
        Ok(json!({"code":200,"data":rows}))
    }
}

async fn replace(client: &MusicClient, epoch: &str, ids: &[&str]) -> PlayerView {
    client
        .play_selection(
            epoch,
            PlaySelection::Songs {
                ids: ids.iter().map(ToString::to_string).collect(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap()
}

async fn marker(client: &MusicClient, epoch: &str, view: &PlayerView) -> Result<Option<f64>> {
    client
        .player_chorus(epoch, view.play_id.as_deref().unwrap())
        .await
}

#[tokio::test]
async fn actor_queue_reuses_chorus_points_across_selection_and_preserves_single_route() {
    let (_dir, api, _session, client, _) = fixture();
    api.chorus.credentials.store(1, Ordering::SeqCst);
    let one = client.play_selection("1", songs("1")).await.unwrap();
    assert_eq!(marker(&client, "1", &one).await.unwrap(), Some(9.0));
    assert_eq!(*api.chorus.singles.lock().unwrap(), ["1"]);
    assert!(api.chorus.calls.lock().unwrap().is_empty());

    let first = replace(&client, "1", &["1", "2"]).await;
    assert_eq!(marker(&client, "1", &first).await.unwrap(), Some(2.0));
    let second = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(marker(&client, "1", &second).await.unwrap(), Some(3.0));
    let first_again = client
        .player_action("1", PlayerAction::Previous)
        .await
        .unwrap();
    assert_eq!(marker(&client, "1", &first_again).await.unwrap(), Some(2.0));
    assert_eq!(*api.chorus.calls.lock().unwrap(), [vec!["1", "2"]]);
    assert_eq!(*api.chorus.singles.lock().unwrap(), ["1"]);

    let inserted = client
        .player_action(
            "1",
            PlayerAction::SearchNextPlay {
                id: "3".into(),
                quality: Quality::Standard,
                expected_queue_revision: first_again.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(inserted.play_id, first_again.play_id);
    assert_ne!(inserted.queue_revision, first_again.queue_revision);
    assert_eq!(marker(&client, "1", &inserted).await.unwrap(), Some(2.0));
    assert_eq!(
        *api.chorus.calls.lock().unwrap(),
        [vec!["1", "2"], vec!["3"], vec!["1", "2"]]
    );
}

#[tokio::test]
async fn chorus_starts_the_next_batch_before_the_first_response() {
    let (_dir, api, _session, client, _) = fixture();
    api.chorus.credentials.store(1, Ordering::SeqCst);
    let first = replace(&client, "1", &["1", "2"]).await;
    api.chorus.hold_first.store(true, Ordering::SeqCst);
    let inserted = client
        .player_action(
            "1",
            PlayerAction::SearchNextPlay {
                id: "3".into(),
                quality: Quality::Standard,
                expected_queue_revision: first.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    let read = tokio::spawn(async move { marker(&client, "1", &inserted).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        api.chorus.second_started.notified(),
    )
    .await
    .expect("the other trial bucket must dispatch before the held first response");
    assert_eq!(
        *api.chorus.calls.lock().unwrap(),
        [vec!["3"], vec!["1", "2"]]
    );
    assert!(
        !read.is_finished(),
        "an incomplete map cannot reach the player"
    );
    api.chorus.release.notify_one();
    assert_eq!(read.await.unwrap().unwrap(), Some(2.0));
}

#[tokio::test]
async fn old_queue_or_credential_reply_cannot_populate_the_current_cache() {
    let (_dir, api, session, client, _) = fixture();
    api.chorus.credentials.store(1, Ordering::SeqCst);
    let first = replace(&client, "1", &["1", "2"]).await;
    api.chorus.hold_first.store(true, Ordering::SeqCst);
    let old = tokio::spawn({
        let client = client.clone();
        async move { marker(&client, "1", &first).await }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        api.chorus.started.notified(),
    )
    .await
    .unwrap();

    let new = replace(&client, "1", &["1", "3"]).await;
    api.chorus.credentials.store(2, Ordering::SeqCst);
    assert_eq!(marker(&client, "1", &new).await.unwrap(), Some(3.0));
    api.chorus.release.notify_one();
    assert!(matches!(old.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(marker(&client, "1", &new).await.unwrap(), Some(3.0));
    assert_eq!(
        *api.chorus.calls.lock().unwrap(),
        [vec!["1", "2"], vec!["1", "3"]]
    );

    // An old epoch cannot reach even the unbatched endpoint.
    let owner = session.token().unwrap();
    session
        .adopt(api.clone(), json!({"userId":10}), &owner)
        .unwrap();
    assert!(matches!(
        marker(&client, "1", &new).await,
        Err(Error::StaleResponse)
    ));
    assert!(api.chorus.singles.lock().unwrap().is_empty());
}

fn persist_queue(path: &Path, owner: &str, ids: &[&str]) {
    let store = cumulus_core::account::profiles::ProfileRegistry::open(
        path,
        cumulus_core::protocol::http::ApiOrigin::Pc,
    )
    .unwrap()
    .store();
    PlaybackQueue::new(Some(store), owner)
        .unwrap()
        .replace(
            ids.iter().map(|id| json!({"id":id,"dt":120000})).collect(),
            0,
            json!({"kind":"songs"}),
        )
        .unwrap();
}

async fn first_batch(api: &Backend) {
    tokio::time::timeout(Duration::from_secs(2), api.called.notified())
        .await
        .expect("native trigger dispatched the bound grouped read");
}

async fn reverse_queue(client: &MusicClient, revision: String) -> PlayerView {
    let mut order: Vec<_> = client
        .player_queue("1")
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|entry| entry.entry_id)
        .collect();
    order.reverse();
    client
        .player_action(
            "1",
            PlayerAction::QueueFor {
                expected_queue_revision: revision,
                expected_room_id: None,
                edit: QueueEdit::Reorder { entry_ids: order },
            },
        )
        .await
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn startup_dispatches_at_one_second_while_the_decoder_source_is_held() {
    let (dir, api, _session, client, _) = fixture();
    persist_queue(dir.path(), "9", &["1", "2"]);
    api.chorus.credentials.store(1, Ordering::SeqCst);
    api.hold_url_id.store(1, Ordering::SeqCst);
    let before = client.player_snapshot("1").await.unwrap();
    assert_eq!(before.state, PlaybackStatus::Idle);
    let started = Instant::now();
    let restore = tokio::spawn({
        let client = client.clone();
        async move { client.player_action("1", PlayerAction::Startup).await }
    });
    api.url_started.notified().await;
    let preparing = client.player_snapshot("1").await.unwrap();
    assert_eq!(preparing.state, PlaybackStatus::Preparing);
    // The startup callback reads the queue at the deadline, not the initial
    // snapshot. Reordering here must neither load another source nor read early.
    let reordered = reverse_queue(&client, preparing.queue_revision.clone()).await;
    assert_ne!(reordered.queue_revision, preparing.queue_revision);
    assert_eq!(reordered.play_id, preparing.play_id);
    tokio::time::advance(Duration::from_millis(999)).await;
    assert!(api.chorus.calls.lock().unwrap().is_empty());
    tokio::time::advance(Duration::from_millis(1)).await;
    first_batch(&api.chorus).await;
    assert_eq!(
        api.chorus.dispatch_at.lock().unwrap().as_slice(),
        [started + Duration::from_secs(1)]
    );
    assert!(!restore.is_finished());
    api.url_release.notify_one();
    let loaded = restore.await.unwrap().unwrap();
    assert_eq!(loaded.state, PlaybackStatus::Paused);
    assert_eq!(marker(&client, "1", &loaded).await.unwrap(), Some(2.0));
    assert_eq!(*api.chorus.calls.lock().unwrap(), [vec!["2", "1"]]);
    assert!(api.chorus.singles.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_already_loaded_marker_joins_the_startup_deadline() {
    let (dir, api, _session, client, _) = fixture();
    persist_queue(dir.path(), "9", &["1", "2"]);
    api.chorus.credentials.store(1, Ordering::SeqCst);
    api.hold_url_id.store(1, Ordering::SeqCst);
    let _ = client.player_snapshot("1").await.unwrap();
    let restore = tokio::spawn({
        let client = client.clone();
        async move { client.player_action("1", PlayerAction::Startup).await }
    });
    api.url_started.notified().await;
    let start = Instant::now();
    api.url_release.notify_one();
    let loaded = restore.await.unwrap().unwrap();
    assert!(Instant::now() < start + Duration::from_secs(1));
    let read = tokio::spawn({
        let client = client.clone();
        async move { marker(&client, "1", &loaded).await }
    });
    tokio::time::advance(Duration::from_millis(500)).await;
    assert!(!read.is_finished());
    assert!(api.chorus.calls.lock().unwrap().is_empty());
    assert_eq!(read.await.unwrap().unwrap(), Some(2.0));
    assert_eq!(*api.chorus.calls.lock().unwrap(), [vec!["1", "2"]]);
    assert!(api.chorus.singles.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn startup_does_not_adopt_a_replacement_account_api_or_cookie() {
    enum Change {
        Owner,
        Api,
        Cookie,
    }
    for change in [Change::Owner, Change::Api, Change::Cookie] {
        let (dir, api, session, client, _) = fixture();
        persist_queue(dir.path(), "9", &["1", "2"]);
        persist_queue(dir.path(), "10", &["2", "3"]);
        api.chorus.credentials.store(1, Ordering::SeqCst);
        api.hold_url_id.store(1, Ordering::SeqCst);
        let restore = tokio::spawn({
            let client = client.clone();
            async move { client.player_action("1", PlayerAction::Startup).await }
        });
        api.url_started.notified().await;
        tokio::time::advance(Duration::from_millis(500)).await;
        let (epoch, current_api, ids) = match change {
            Change::Owner => {
                let owner = session.token().unwrap();
                session
                    .adopt(api.clone(), json!({"userId":10}), &owner)
                    .unwrap();
                ("2", api.clone(), vec!["2", "3"])
            }
            Change::Api => {
                let replacement = Arc::new(super::Backend::default());
                replacement.chorus.credentials.store(2, Ordering::SeqCst);
                let owner = session.token().unwrap();
                session
                    .adopt(replacement.clone(), json!({"userId":9}), &owner)
                    .unwrap();
                ("2", replacement, vec!["1", "2"])
            }
            Change::Cookie => {
                api.chorus.credentials.store(2, Ordering::SeqCst);
                ("1", api.clone(), vec!["1", "2"])
            }
        };
        api.hold_url_id.store(0, Ordering::SeqCst);
        api.url_release.notify_one();
        let _ = restore.await.unwrap();
        if epoch == "2" {
            // A new session restores its own queue but cannot restart the
            // already consumed process-launch chorus timer.
            client
                .player_action(epoch, PlayerAction::Startup)
                .await
                .unwrap();
        }
        tokio::time::advance(Duration::from_secs(2)).await;
        let current = client.player_snapshot(epoch).await.unwrap();
        assert!(api.chorus.calls.lock().unwrap().is_empty());
        assert!(current_api.chorus.calls.lock().unwrap().is_empty());
        assert_eq!(marker(&client, epoch, &current).await.unwrap(), Some(3.0));
        assert_eq!(*current_api.chorus.calls.lock().unwrap(), [ids]);
    }
}

#[tokio::test(start_paused = true)]
async fn accepted_next_play_fills_without_a_renderer_read_or_decoder_replacement() {
    let (_dir, api, _session, client, _) = fixture();
    api.chorus.credentials.store(1, Ordering::SeqCst);
    let loaded = replace(&client, "1", &["1", "2"]).await;
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let attempt = |id: &str, revision: String| PlayerAction::NextPlay {
        id: id.into(),
        quality: Quality::Standard,
        expected_queue_revision: revision,
    };
    assert!(matches!(
        client.player_action("1", attempt("3", "0".into())).await,
        Err(Error::QueueChanged)
    ));
    let no_op = client
        .player_action("1", attempt("1", paused.queue_revision.clone()))
        .await
        .unwrap();
    assert_eq!(no_op.queue_revision, paused.queue_revision);
    let reordered = reverse_queue(&client, paused.queue_revision).await;
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(api.chorus.calls.lock().unwrap().is_empty());

    let inserted = client
        .player_action("1", attempt("3", reordered.queue_revision))
        .await
        .unwrap();
    assert_eq!(inserted.state, PlaybackStatus::Paused);
    assert_eq!(inserted.play_id, loaded.play_id);
    assert_eq!(inserted.entry_id, loaded.entry_id);
    first_batch(&api.chorus).await;
    assert_eq!(*api.chorus.calls.lock().unwrap(), [vec!["2", "1", "3"]]);
    assert_eq!(marker(&client, "1", &inserted).await.unwrap(), Some(2.0));
    assert_eq!(api.chorus.calls.lock().unwrap().len(), 1);
    assert!(api.chorus.singles.lock().unwrap().is_empty());
}
