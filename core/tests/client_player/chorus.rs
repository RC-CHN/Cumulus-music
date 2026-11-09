use super::{fixture, songs};
use cumulus_core::{
    Error, Result,
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction, PlayerView},
    },
    membership::Quality,
    protocol::api::{PlaybackChorusPermission, PlaybackChorusRead},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct Backend {
    credentials: AtomicU64,
    hold_first: AtomicBool,
    started: Notify,
    release: Notify,
    calls: Mutex<Vec<Vec<String>>>,
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
        self.calls.lock().unwrap().push(ids.clone());
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
