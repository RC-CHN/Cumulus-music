//! Exercises the existing native playback hydration and read-only purchase
//! observation against sealed responses. No download or playlist APIs allowed.
use super::*;
use cumulus_core::{client::CurrentSongPurchase, storage::StateStore};

#[derive(Default)]
pub(super) struct Server {
    pub(super) revision: AtomicU64,
    pub(super) mode: AtomicU64,
    pub(super) observed: Mutex<Vec<Vec<String>>>,
}

impl Server {
    pub(super) fn revision(&self) -> Option<String> {
        let revision = self.revision.load(Ordering::SeqCst);
        (revision != 0).then(|| format!("synthetic-player-purchase-{revision}"))
    }

    pub(super) fn annotate(&self, ids: &[String], mut response: Value) -> (Value, Option<String>) {
        let revision = self.revision();
        if revision.is_none() {
            return (response, None);
        }
        self.observed.lock().unwrap().push(ids.to_vec());
        let mode = self.mode.load(Ordering::SeqCst);
        let mut rows: Vec<_> = ids
            .iter()
            .map(|id| {
                json!({
                    "id": id,
                    "st": if mode == 6 { -200 } else { 0 },
                    "fee": if mode == 9 { 0 } else { 4 },
                    "payed": match mode { 1 => 1, 9 => 0, _ => 2 },
                    "dl": 128000,
                    "maxbr": 999000,
                    "flag": 1 << 12,
                    "cs": if mode == 5 { 1 } else { 0 },
                    "subp": if mode == 8 { -1 } else { 0 },
                })
            })
            .collect();
        if mode == 2 {
            rows.push(rows[0].clone());
        } else if mode == 3 {
            rows[0]["id"] = json!("999");
        } else if mode == 4 {
            let songs = response["songs"].as_array_mut().unwrap();
            songs.push(songs[0].clone());
        }
        response["privileges"] = json!(rows);
        (response, if mode == 7 { None } else { revision })
    }
}

fn observe(
    client: &MusicClient,
    view: &PlayerView,
) -> cumulus_core::Result<Option<CurrentSongPurchase>> {
    client.current_player_song_purchase(
        &view.epoch,
        view.play_id.as_deref().unwrap(),
        view.entry_id.as_deref().unwrap(),
    )
}

#[tokio::test]
async fn selected_purchase_reuses_detail_without_leaking_rights_or_starting_download() {
    let (dir, api, _, client, _) = fixture();
    api.player_song_purchase.revision.store(1, Ordering::SeqCst);
    let selected = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into()],
                index: 1,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(selected.track.as_ref().unwrap().id, "2");
    let before = api.calls.lock().unwrap().clone();
    assert_eq!(
        before
            .iter()
            .filter(|path| path.as_str() == "/api/v3/song/detail")
            .count(),
        1,
    );
    let purchase = observe(&client, &selected).unwrap().unwrap();
    assert_eq!(purchase.song_id, "2");
    assert!(purchase.picker_collectable);
    assert_eq!(
        purchase.qualities,
        [
            Quality::HiRes,
            Quality::Lossless,
            Quality::ExHigh,
            Quality::Standard
        ]
    );
    assert_eq!(*api.calls.lock().unwrap(), before);
    assert_eq!(
        *api.player_song_purchase.observed.lock().unwrap(),
        [vec!["1", "2"]]
    );

    let store = StateStore::open(dir.path()).unwrap();
    let queue = store.get("9", "playback", "queue").unwrap().unwrap();
    let queue = queue.to_string();
    for private in [
        "synthetic-player-purchase",
        "\"payed\"",
        "\"dl\"",
        "\"maxbr\"",
        "\"flag\"",
    ] {
        assert!(!queue.contains(private), "persisted {private}");
    }
    let projected = serde_json::to_string(&selected).unwrap();
    assert!(!projected.contains("purchase") && !projected.contains("synthetic-player-purchase"));
}

#[tokio::test]
async fn vip_duplicate_wrong_private_blocked_and_unsigned_detail_do_not_vouch_for_purchase() {
    let (_dir, api, _, client, _) = fixture();
    api.player_song_purchase.revision.store(1, Ordering::SeqCst);
    for mode in 1..=7 {
        api.player_song_purchase.mode.store(mode, Ordering::SeqCst);
        let selected = client.play_selection("1", songs("1")).await.unwrap();
        assert!(
            observe(&client, &selected).unwrap().is_none(),
            "mode {mode}"
        );
    }
    api.player_song_purchase.mode.store(8, Ordering::SeqCst);
    let selected = client.play_selection("1", songs("1")).await.unwrap();
    let purchase = observe(&client, &selected).unwrap().unwrap();
    assert!(!purchase.picker_collectable);
    assert!(!purchase.qualities.is_empty());
}

#[tokio::test]
async fn changed_credentials_and_play_instance_retire_the_observation() {
    let (_dir, api, session, client, _) = fixture();
    let wire = &api.player_song_purchase;
    wire.revision.store(1, Ordering::SeqCst);
    let original = client.play_selection("1", songs("1")).await.unwrap();
    assert!(observe(&client, &original).unwrap().is_some());
    wire.revision.store(2, Ordering::SeqCst);
    assert!(matches!(
        observe(&client, &original),
        Err(Error::StaleResponse)
    ));
    wire.revision.store(1, Ordering::SeqCst);
    assert!(observe(&client, &original).unwrap().is_none());

    let replacement = client.play_selection("1", songs("1")).await.unwrap();
    assert_ne!(original.play_id, replacement.play_id);
    assert!(matches!(
        observe(&client, &original),
        Err(Error::StaleResponse)
    ));
    assert!(observe(&client, &replacement).unwrap().is_some());
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        observe(&client, &replacement),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn delayed_detail_cannot_replace_newer_current_purchase() {
    let (_dir, api, _, client, _) = fixture();
    api.player_song_purchase.revision.store(1, Ordering::SeqCst);
    api.hold_id.store(1, Ordering::SeqCst);
    let original = tokio::spawn({
        let client = client.clone();
        async move { client.play_selection("1", songs("1")).await }
    });
    api.started.notified().await;
    let replacement = client.play_selection("1", songs("2")).await.unwrap();
    assert_eq!(
        observe(&client, &replacement).unwrap().unwrap().song_id,
        "2"
    );
    api.release.notify_one();
    assert!(matches!(original.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        observe(&client, &replacement).unwrap().unwrap().song_id,
        "2"
    );
}
