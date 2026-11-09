use super::fixture;
use cumulus_core::{
    client::dto::{PlaySelection, PlayerAction, QueueClickPreferencesCommand, SongRowSelection},
    membership::Quality,
    protocol::api::identifier,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const LIKED: &str = "7001";
const OTHER_LIKED: &str = "7002";
const RADAR: &str = "9102252203";
const UNRELATED_SPECIAL: &str = "7003";

#[derive(Default)]
pub(super) struct Fixture {
    enabled: AtomicBool,
    conflicting_owner: AtomicBool,
    media: Mutex<Vec<(String, Value)>>,
}

impl Fixture {
    pub(super) fn list(&self, payload: &Value) -> Value {
        assert!(self.enabled.load(Ordering::SeqCst));
        assert_eq!(
            payload,
            &json!({"uid":"9","includeVideo":true,"limit":200,"offset":0})
        );
        json!({"code":200,"more":false,"playlist":[
            {"id":LIKED,"name":"我的喜欢","userId":9,"specialType":5},
            {"id":OTHER_LIKED,"name":"另一个账号的喜欢","userId":10,"specialType":5}
        ]})
    }

    pub(super) fn detail(&self, payload: &Value) -> Option<Value> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        let id = payload["id"].as_str()?;
        let (owner, special, first) = match id {
            LIKED => (9, 5, 301),
            OTHER_LIKED => (10, 5, 305),
            RADAR => (11, 0, 401),
            UNRELATED_SPECIAL => (12, 100, 411),
            _ => return None,
        };
        let creator = if id == LIKED && self.conflicting_owner.load(Ordering::SeqCst) {
            owner + 1
        } else {
            owner
        };
        let mut tracks = vec![json!({"id":first}), json!({"id":first+1})];
        if id == UNRELATED_SPECIAL {
            // Reuse an old occurrence with the default context rather than
            // testing insertion only with brand-new entries.
            tracks.push(json!({"id":401}));
        }
        Some(json!({"code":200,"playlist":{
            "id":id,"name":"看起来像雷达的标题","userId":owner,"specialType":special,
            "creator":{"userId":creator},"trackIds":tracks
        }}))
    }

    pub(super) fn record_media(&self, ids: &[Value], mode: &Value) {
        if self.enabled.load(Ordering::SeqCst) {
            self.media
                .lock()
                .unwrap()
                .extend(ids.iter().map(|id| (identifier(id).unwrap(), mode.clone())));
        }
    }

    fn clear(&self) {
        self.media.lock().unwrap().clear();
    }

    fn modes(&self, id: &str) -> Vec<i64> {
        self.media
            .lock()
            .unwrap()
            .iter()
            .filter(|(actual, _)| actual == id)
            .map(|(_, mode)| mode.as_i64().unwrap())
            .collect()
    }
}

fn playlist(id: &str) -> PlaySelection {
    PlaySelection::Playlist {
        id: id.into(),
        start_id: None,
        revision: None,
        quality: Quality::Standard,
    }
}

#[tokio::test]
async fn liked_mode_requires_current_host_list_and_does_not_cross_accounts() {
    let (_dir, api, session, client, _count) = fixture();
    api.playlist_trial.enabled.store(true, Ordering::SeqCst);
    client.play_selection("1", playlist(LIKED)).await.unwrap();
    assert_eq!(api.playlist_trial.modes("301"), [-1]);
    assert!(
        !api.calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p == "/api/user/playlist")
    );

    let host = client.playlists().await.unwrap();
    assert!(
        host.value
            .iter()
            .any(|p| p.id == LIKED && p.favorite && p.owned)
    );
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    api.playlist_trial.clear();
    client.play_selection("1", playlist(LIKED)).await.unwrap();
    assert_eq!(api.playlist_trial.modes("301"), [43]);

    client
        .play_selection("1", playlist(OTHER_LIKED))
        .await
        .unwrap();
    assert_eq!(api.playlist_trial.modes("305"), [-1]);
    api.playlist_trial.clear();
    api.playlist_trial
        .conflicting_owner
        .store(true, Ordering::SeqCst);
    // Use a fresh URL-cache key to observe the contradictory detail's request.
    client
        .play_selection(
            "1",
            PlaySelection::Playlist {
                id: LIKED.into(),
                start_id: None,
                revision: None,
                quality: Quality::Higher,
            },
        )
        .await
        .unwrap();
    assert_eq!(api.playlist_trial.modes("301"), [-1]);

    api.playlist_trial
        .conflicting_owner
        .store(false, Ordering::SeqCst);
    api.playlist_trial.clear();
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    client.play_selection("2", playlist(LIKED)).await.unwrap();
    assert_eq!(api.playlist_trial.modes("301"), [-1]);
}

#[tokio::test]
async fn radar_context_is_bound_to_the_recovered_id_for_batch_and_queued_play() {
    let (_dir, api, _session, client, _count) = fixture();
    api.playlist_trial.enabled.store(true, Ordering::SeqCst);
    let current = client
        .play_selection("1", playlist(UNRELATED_SPECIAL))
        .await
        .unwrap();
    assert_eq!(api.playlist_trial.modes("411"), [-1]);
    let before = client.player_queue("1").await.unwrap();
    let before_id = before
        .tracks
        .iter()
        .find(|item| item.track.id == "401")
        .unwrap()
        .entry_id
        .clone();
    let queued = client
        .player_action(
            "1",
            PlayerAction::QueuePlaylist {
                id: RADAR.into(),
                quality: Quality::Standard,
                expected_queue_revision: current.queue_revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(queued.play_id, current.play_id);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|item| item.track.id.as_str())
            .collect::<Vec<_>>(),
        ["411", "401", "402", "412"]
    );
    assert_eq!(queue.tracks[1].entry_id, before_id);
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(next.track.unwrap().id, "401");
    assert!(api.playlist_trial.modes("401").iter().all(|m| *m == 37));
    assert!(!api.playlist_trial.modes("401").is_empty());

    let revision = hex::encode(Sha256::digest(
        serde_json::to_vec(&json!([RADAR, [{"id":401}, {"id":402}]])).unwrap(),
    ));
    client.player_action("1", PlayerAction::Stop).await.unwrap();
    api.playlist_trial.clear();
    let picked = client
        .play_selection(
            "1",
            PlaySelection::PlaylistBatch {
                id: RADAR.into(),
                ids: vec!["402".into()],
                revision,
                // A prior next may already have prefetched standard audio.
                quality: Quality::Lossless,
            },
        )
        .await
        .unwrap();
    assert_eq!(picked.track.unwrap().id, "402");
    assert_eq!(api.playlist_trial.modes("402"), [37]);
}

#[tokio::test]
async fn retained_playlist_row_rebinds_an_old_noncurrent_occurrence_without_moving_it() {
    let (_dir, api, _session, client, _count) = fixture();
    api.playlist_trial.enabled.store(true, Ordering::SeqCst);
    client
        .queue_click_preferences_command(
            "1",
            QueueClickPreferencesCommand::Change {
                expected_revision: "0".into(),
                replace_queue: false,
            },
        )
        .unwrap();
    client
        .play_selection("1", playlist(UNRELATED_SPECIAL))
        .await
        .unwrap();
    let entries = |queue: cumulus_core::client::dto::QueueView| {
        queue
            .tracks
            .into_iter()
            .map(|item| item.entry_id)
            .collect::<Vec<_>>()
    };
    let before = entries(client.player_queue("1").await.unwrap());
    let selection = || PlaySelection::SongRowDoubleClick {
        selection: SongRowSelection::Playlist {
            id: RADAR.into(),
            start_id: "401".into(),
            revision: None,
            quality: Quality::Higher,
        },
    };
    let selected = client.play_selection("1", selection()).await.unwrap();
    assert_eq!(
        selected.entry_id.as_deref(),
        before.last().map(String::as_str)
    );
    assert_eq!(api.playlist_trial.modes("401"), [37]);
    assert_eq!(entries(client.player_queue("1").await.unwrap()), before);
    let again = client.play_selection("1", selection()).await.unwrap();
    assert_eq!(again.play_id, selected.play_id);
    assert_eq!(api.playlist_trial.modes("401"), [37]);
}
