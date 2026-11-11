use super::*;

#[derive(Default)]
struct BackWire {
    wire: Wire,
    hide_second: AtomicBool,
    reorder: AtomicBool,
}

impl Api for BackWire {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mut reply = self.wire.request(path, args, cache).await?;
            if path == "/api/v6/playlist/detail"
                && reply["playlist"]["id"] == "73"
                && self.reorder.load(Ordering::Acquire)
            {
                reply["playlist"]["trackIds"]
                    .as_array_mut()
                    .unwrap()
                    .swap(1, 2);
            }
            if path == "/api/v3/song/detail" && self.hide_second.load(Ordering::Acquire) {
                for song in reply["songs"].as_array_mut().unwrap() {
                    if song["id"] == "12" {
                        song["t"] = json!(1);
                    }
                }
            }
            Ok(reply)
        })
    }
}

async fn page(client: &MusicClient, id: &str, limit: usize) -> TrackPage {
    client
        .playlist_song_menu_page_at(id, request(0, limit, None), None)
        .await
        .unwrap()
        .value
}

async fn second(client: &MusicClient, first: &TrackPage) -> TrackPage {
    client
        .playlist_song_menu_page_at(
            "73",
            request(1, 1, Some(first)),
            first
                .playlist_local
                .as_ref()
                .map(|local| local.revision.clone()),
        )
        .await
        .unwrap()
        .value
}

async fn rejected(client: &MusicClient, wire: &BackWire, receipt: &str) {
    let calls = wire.wire.calls.lock().unwrap().len();
    assert!(matches!(
        client.playlist_song_row_comment_count("1", receipt).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(wire.wire.calls.lock().unwrap().len(), calls);
}

#[tokio::test(start_paused = true)]
async fn back_retains_only_a_matching_previously_observed_suffix_and_expiry_is_bounded() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(BackWire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = page(&client, "73", 1).await;
    let suffix = second(&client, &first).await;
    let receipt_first = receipt(&first, 0);
    let receipt_suffix = receipt(&suffix, 0);
    assert_eq!(suffix.tracks[0].id, "12");

    page(&client, "74", 1).await;
    rejected(&client, &wire, receipt_suffix).await;
    let restored = page(&client, "73", 1).await;
    assert_eq!(restored.revision, first.revision);
    rejected(&client, &wire, receipt_first).await;
    let calls = wire.wire.calls.lock().unwrap().len();
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt_suffix)
            .await
            .unwrap()
            .value,
        Some(0)
    );
    assert_eq!(
        &wire.wire.calls.lock().unwrap()[calls..],
        &[(
            COUNT.into(),
            json!({"resourceType":4,"resourceIds":"[\"12\"]"}),
            false
        )]
    );

    // Even under an unchanged fresh playlist, an archived capability has a
    // bounded lifetime, and its failure performs no count or re-fetch.
    tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
    rejected(&client, &wire, receipt_suffix).await;
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt(&restored, 0))
            .await
            .unwrap()
            .value,
        Some(36)
    );
}

#[tokio::test]
async fn back_never_overrides_newly_observed_hidden_rows_or_new_playlist_revision() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(BackWire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = page(&client, "73", 1).await;
    let suffix = second(&client, &first).await;
    let receipt_suffix = receipt(&suffix, 0);
    page(&client, "74", 1).await;

    wire.hide_second.store(true, Ordering::Release);
    let same = page(&client, "73", 1).await;
    assert_eq!(same.revision, first.revision);
    let now_hidden = second(&client, &same).await;
    assert_eq!(
        now_hidden.tracks[0].playlist_song_comment_menu.as_deref(),
        Some(&PlaylistSongCommentMenu::Hidden)
    );
    rejected(&client, &wire, receipt_suffix).await;

    wire.reorder.store(true, Ordering::Release);
    let changed = page(&client, "73", 1).await;
    assert_ne!(changed.revision, first.revision);
    rejected(&client, &wire, receipt_suffix).await;
}

#[tokio::test(start_paused = true)]
async fn back_revalidation_cannot_publish_a_count_from_the_departed_visit() {
    let root = tempfile::tempdir().unwrap();
    let wire = Arc::new(BackWire::default());
    let (client, _, _) = open(root.path(), wire.clone(), Some(9));
    let first = page(&client, "73", 1).await;
    let suffix = second(&client, &first).await;
    let receipt_suffix = receipt(&suffix, 0);
    wire.wire.hold.store(true, Ordering::Release);
    let pending = client.playlist_song_row_comment_count("1", receipt_suffix);
    tokio::pin!(pending);
    tokio::select! {
        _ = wire.wire.entered.notified() => (),
        _ = &mut pending => panic!("native count did not wait for its fixture reply"),
    }
    page(&client, "74", 1).await;
    page(&client, "73", 1).await;
    wire.wire.hold.store(false, Ordering::Release);
    wire.wire.release.notify_one();
    assert!(matches!(pending.await, Err(Error::QueueChanged)));
    assert_eq!(
        client
            .playlist_song_row_comment_count("1", receipt_suffix)
            .await
            .unwrap()
            .value,
        Some(0)
    );
}
