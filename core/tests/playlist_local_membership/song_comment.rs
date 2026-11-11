use super::*;
use cumulus_core::client::dto::{PlaylistDurationSource, PlaylistSongCommentMenu, TrackPage};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

#[path = "song_comment_back.rs"]
mod back;

const COUNT: &str = "/api/resource/commentInfo/list";

#[derive(Default)]
struct Wire {
    calls: Mutex<Vec<(String, Value, bool)>>,
    offline: AtomicBool,
    hold: AtomicBool,
    entered: Notify,
    release: Notify,
    hold_duration: AtomicBool,
    duration_entered: Notify,
    duration_release: Notify,
}

impl Api for Wire {
    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), args.clone(), cached));
            match path {
                "/api/v6/playlist/detail" => {
                    if self.offline.load(Ordering::Acquire) {
                        return Err(Error::Transport("sealed playlist transport cut".into()));
                    }
                    let id = args["id"].as_str().unwrap();
                    let (special, tracks) = match id {
                        "73" => (0, vec![11, 12, 13, 14, 15, 77, 42]),
                        "74" => (5, vec![51]),
                        _ => panic!("unexpected synthetic playlist: {id}"),
                    };
                    Ok(json!({"code":200,"playlist":{
                        "id":id,"name":format!("Synthetic {id}"),"userId":9,"privacy":0,
                        "specialType":special,"trackCount":tracks.len(),"updateTime":1,
                        "trackIds":tracks.iter().map(|id|json!({"id":id,"v":if *id==77 {-1}else{0}})).collect::<Vec<_>>()
                    }}))
                }
                "/api/v3/song/detail" => {
                    if args["trialMode"] == -1 && self.hold_duration.load(Ordering::Acquire) {
                        self.duration_entered.notify_one();
                        self.duration_release.notified().await;
                    }
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    assert!(!rows.iter().any(|row| row["id"] == "77"));
                    let songs: Vec<_> = rows
                        .iter()
                        .filter_map(|row| {
                            let id = row["id"].as_str().unwrap();
                            if id == "15" {
                                return None;
                            }
                            Some(json!({"id":id,"name":format!("Song {id}"),"dt":60000,
                            "t":if id=="13" {1}else{0},"fee":if id=="12" {1}else{0},
                            "privilege":{"id":id,"st":if id=="12" {-100}else{0},"cs":id=="14"}}))
                        })
                        .collect();
                    Ok(json!({"code":200,"songs":songs}))
                }
                COUNT => {
                    assert!(!cached && args.as_object().unwrap().len() == 2);
                    assert_eq!(args["resourceType"], 4);
                    let ids: Vec<String> =
                        serde_json::from_str(args["resourceIds"].as_str().unwrap()).unwrap();
                    let [id] = ids.as_slice() else {
                        panic!("only the clicked song may be read")
                    };
                    if self.hold.load(Ordering::Acquire) {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    let (thread, count) = match id.as_str() {
                        "11" => ("R_SO_4_11", 36),
                        "12" => ("R_SO_4_12", 0),
                        "42" => ("R_SO_4_404", 999),
                        "51" => return Err(Error::Transport("optional count unavailable".into())),
                        _ => {
                            panic!("unobserved or ineligible song leaked onto the count wire: {id}")
                        }
                    };
                    Ok(json!({"code":200,"data":[{"threadId":thread,"commentCount":count}]}))
                }
                _ => panic!("unexpected synthetic request: {path}"),
            }
        })
    }
}

fn request(offset: usize, limit: usize, before: Option<&TrackPage>) -> PageRequest {
    PageRequest {
        offset,
        limit,
        revision: before.and_then(|page| page.revision.clone()),
    }
}

fn receipt(page: &TrackPage, index: usize) -> &str {
    match page.tracks[index].playlist_song_comment_menu.as_deref() {
        Some(PlaylistSongCommentMenu::Eligible { receipt }) => receipt,
        other => panic!("expected an eligible playlist row, received {other:?}"),
    }
}

fn duration_source(page: &TrackPage) -> PlaylistDurationSource {
    PlaylistDurationSource {
        revision: page.revision.clone().unwrap(),
        local_revision: page
            .playlist_local
            .as_ref()
            .map(|local| local.revision.clone()),
        cached_at_ms: page
            .playlist_offline
            .as_ref()
            .map(|cache| cache.cached_at_ms),
    }
}

#[tokio::test]
async fn live_playlist_comment_receipt_binds_the_clicked_public_song_without_another_source_read() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(Wire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = client
        .playlist_song_menu_page_at("73", request(0, 6, None), None)
        .await
        .unwrap()
        .value;
    assert_eq!(
        first
            .tracks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["11", "12", "13", "14", "15", "77"]
    );
    let exact = receipt(&first, 0);
    assert_eq!(
        serde_json::to_value(&first.tracks[0]).unwrap()["playlistSongCommentMenu"],
        json!({"kind":"eligible","receipt":exact})
    );
    let hidden = PlaylistSongCommentMenu::Hidden;
    assert!(
        first.tracks[2..]
            .iter()
            .all(|row| row.playlist_song_comment_menu.as_deref() == Some(&hidden))
    );
    assert_eq!(first.tracks[1].fee, 1);
    let background = client
        .playlist_page_at("73", request(0, 1, None), None)
        .await
        .unwrap()
        .value;
    assert!(background.tracks[0].playlist_song_comment_menu.is_none());
    let baseline = wire.calls.lock().unwrap().len();
    assert!(matches!(
        client.playlist_song_row_comment_count("1", "11").await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client.playlist_song_row_comment_count("0", exact).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(wire.calls.lock().unwrap().len(), baseline);
    let count = client
        .playlist_song_row_comment_count("1", exact)
        .await
        .unwrap();
    assert_eq!((count.epoch.as_str(), count.value), ("1", Some(36)));
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&first, 1))
            .await
            .unwrap()
            .value,
        Some(0)
    );
    assert_eq!(
        &wire.calls.lock().unwrap()[baseline..],
        &[
            (
                COUNT.into(),
                json!({"resourceType":4,"resourceIds":"[\"11\"]"}),
                false
            ),
            (
                COUNT.into(),
                json!({"resourceType":4,"resourceIds":"[\"12\"]"}),
                false
            ),
        ]
    );
    let full = client
        .playlist_song_menu_page_at("73", request(0, 7, None), None)
        .await
        .unwrap()
        .value;
    assert!(!full.more);
    wire.offline.store(true, Ordering::Release);
    let offline = client
        .playlist_song_menu_page_at("73", request(0, 7, None), None)
        .await
        .unwrap()
        .value;
    assert!(offline.playlist_offline.is_some());
    assert!(
        offline
            .tracks
            .iter()
            .all(|row| row.playlist_song_comment_menu.is_none())
    );
    let sorted_offline = client
        .playlist_duration_prepare("1", "73", duration_source(&offline))
        .await
        .unwrap()
        .value;
    assert!(
        sorted_offline
            .page
            .tracks
            .iter()
            .all(|row| row.playlist_song_comment_menu.is_none())
    );
    let baseline = wire.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .playlist_song_row_comment_count("1", receipt(&full, 0))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(wire.calls.lock().unwrap().len(), baseline);
}

#[tokio::test]
async fn live_playlist_comment_suffix_and_wrong_thread_falls_back() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(Wire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = client
        .playlist_song_menu_page_at("73", request(0, 6, None), None)
        .await
        .unwrap()
        .value;
    let local = first.playlist_local.as_ref().unwrap().revision.clone();
    let suffix = || {
        client.playlist_song_menu_page_at("73", request(6, 1, Some(&first)), Some(local.clone()))
    };
    let old = suffix().await.unwrap().value;
    let new = suffix().await.unwrap().value;
    assert_ne!(receipt(&old, 0), receipt(&new, 0));
    let baseline = wire.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .playlist_song_row_comment_count("1", receipt(&old, 0))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(wire.calls.lock().unwrap().len(), baseline);
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&new, 0))
            .await
            .unwrap()
            .value,
        None
    );
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&first, 0))
            .await
            .unwrap()
            .value,
        Some(36)
    );
    let favorite = client
        .playlist_song_menu_page_at("74", request(0, 1, None), None)
        .await
        .unwrap()
        .value;
    assert!(favorite.playlist.as_ref().unwrap().favorite);
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&favorite, 0))
            .await
            .unwrap()
            .value,
        None
    );
    assert!(matches!(
        client
            .playlist_song_row_comment_count("1", receipt(&new, 0))
            .await,
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn live_duration_hydrates_menu_rows_without_rotating_matching_ordinal_receipts() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(Wire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = client
        .playlist_song_menu_page_at("73", request(0, 2, None), None)
        .await
        .unwrap()
        .value;
    let source = duration_source(&first);
    let sorted = client
        .playlist_duration_prepare("1", "73", source.clone())
        .await
        .unwrap()
        .value;
    assert_eq!(sorted.page.tracks.len(), 7);
    assert_eq!(receipt(&first, 0), receipt(&sorted.page, 0));
    assert_eq!(receipt(&first, 1), receipt(&sorted.page, 1));
    assert!(sorted.page.tracks[2..6].iter().all(|row| {
        row.playlist_song_comment_menu.as_deref() == Some(&PlaylistSongCommentMenu::Hidden)
    }));
    let hydrated = receipt(&sorted.page, 6);
    assert_eq!(sorted.page.tracks[6].id, "42");
    let prior = wire.calls.lock().unwrap().len();
    let again = client
        .playlist_duration_prepare("1", "73", source)
        .await
        .unwrap()
        .value;
    assert_eq!(wire.calls.lock().unwrap().len(), prior);
    assert_eq!(hydrated, receipt(&again.page, 6));
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&again.page, 1))
            .await
            .unwrap()
            .value,
        Some(0)
    );
    // The exact synthetic count for 42 deliberately supplies the wrong thread.
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", hydrated)
            .await
            .unwrap()
            .value,
        None
    );
    let calls = wire.calls.lock().unwrap();
    assert_eq!(calls[prior].1["resourceIds"], "[\"12\"]");
    assert_eq!(calls[prior + 1].1["resourceIds"], "[\"42\"]");
}

#[tokio::test]
async fn late_duration_hydration_cannot_replace_a_new_playlist_menu_visit() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(Wire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = client
        .playlist_song_menu_page_at("73", request(0, 2, None), None)
        .await
        .unwrap()
        .value;
    wire.hold_duration.store(true, Ordering::Release);
    let old = client.playlist_duration_prepare("1", "73", duration_source(&first));
    tokio::pin!(old);
    tokio::select! {
        _ = wire.duration_entered.notified() => (),
        _ = &mut old => panic!("old duration hydration resolved before navigation"),
    }
    let next = client
        .playlist_song_menu_page_at("74", request(0, 1, None), None)
        .await
        .unwrap()
        .value;
    wire.duration_release.notify_one();
    let old = old.await.unwrap().value;
    assert_eq!(old.page.tracks.len(), 7);
    assert!(
        old.page
            .tracks
            .iter()
            .all(|row| row.playlist_song_comment_menu.is_none())
    );
    let prior = wire.calls.lock().unwrap().len();
    assert!(matches!(
        client
            .playlist_song_row_comment_count("1", receipt(&first, 0))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(wire.calls.lock().unwrap().len(), prior);
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&next, 0))
            .await
            .unwrap()
            .value,
        None
    );
    assert_eq!(
        wire.calls.lock().unwrap()[prior].1["resourceIds"],
        "[\"51\"]"
    );
}

#[tokio::test(start_paused = true)]
async fn live_playlist_comment_timeout_and_late_refresh_or_account_replacement_never_publish_old_count()
 {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(Wire::default());
    let (client, session, _) = open(root.path(), wire.clone(), Some(9));
    let first = client
        .playlist_song_menu_page_at("73", request(0, 6, None), None)
        .await
        .unwrap()
        .value;
    wire.hold.store(true, Ordering::Release);
    let before = tokio::time::Instant::now();
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&first, 0))
            .await
            .unwrap()
            .value,
        None
    );
    assert_eq!(before.elapsed(), std::time::Duration::from_millis(1500));
    // Clear the notification from the timed-out request before holding a new one.
    wire.entered.notified().await;
    let old = client.playlist_song_row_comment_count("1", receipt(&first, 0));
    tokio::pin!(old);
    tokio::select! {
        _ = wire.entered.notified() => (),
        _ = &mut old => panic!("held count resolved before refresh"),
    }
    let refreshed = client
        .playlist_song_menu_page_at("73", request(0, 6, None), None)
        .await
        .unwrap()
        .value;
    assert_eq!(first.revision, refreshed.revision);
    assert_ne!(receipt(&first, 0), receipt(&refreshed, 0));
    wire.release.notify_one();
    assert!(matches!(old.await, Err(Error::QueueChanged)));

    let detached = client.playlist_song_row_comment_count("1", receipt(&refreshed, 0));
    tokio::pin!(detached);
    tokio::select! {
        _ = wire.entered.notified() => (),
        _ = &mut detached => panic!("held count resolved before account switch"),
    }
    session.detach().unwrap();
    wire.release.notify_one();
    assert!(matches!(detached.await, Err(Error::StaleResponse)));
}
