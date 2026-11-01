use super::*;
use cumulus_core::download::{
    service::{DownloadDestination, DownloadRunOptions, DownloadService, DownloadStatus},
    transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
};
pub(super) const MEDIA_BYTES: &[u8] = include_bytes!("../fixtures/tag_audio/seed.flac");
pub(super) const MEDIA_URL: &str = "https://m1.music.126.net/cloud-menu-original.flac";
struct Files;
impl MediaBackend for Files {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(request.url.as_str(), MEDIA_URL);
            assert_eq!(request.offset, 0);
            Ok(MediaReply {
                status: 200,
                headers: std::collections::BTreeMap::from([
                    ("content-length".into(), MEDIA_BYTES.len().to_string()),
                    ("etag".into(), "\"cloud-original\"".into()),
                ]),
                body: Box::new(Bytes(0)),
            })
        })
    }
}
struct Bytes(usize);
impl MediaBody for Bytes {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let size = buffer.len().min(MEDIA_BYTES.len() - self.0);
            buffer[..size].copy_from_slice(&MEDIA_BYTES[self.0..self.0 + size]);
            self.0 += size;
            Ok(size)
        })
    }
}
#[tokio::test]
async fn cloud_resource_never_invents_a_public_catalog_identity_or_exposes_native_paths() {
    let f = Fixture::new(5);
    {
        let mut rows = f.api.rows.lock().unwrap();
        rows[0]["simpleSong"]["id"] = json!(9001);
        rows[1]["simpleSong"]["id"] = json!(9002);
        rows[1]["simpleSong"]["t"] = json!(1);
        rows[2]["simpleSong"]["id"] = json!(9003);
        rows[2]["simpleSong"]["privilege"] = json!({"st":-100});
        rows[3]["simpleSong"]["id"] = json!("https://invalid.example/SECRET");
        rows[4]["simpleSong"]["id"] = json!(9005);
        rows[4]["simpleSong"]["t"] = json!("1");
    }
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    for id in 1..=5 {
        let resource = client
            .cloud_resource("1", &id.to_string(), &revision)
            .await
            .unwrap();
        assert_eq!(resource.track.track.id, id.to_string());
        assert_eq!(
            resource.track.public_song_id,
            if id == 1 { Some("9001".into()) } else { None }
        );
        assert!(!resource.local_available);
        let encoded = serde_json::to_string(&resource).unwrap();
        assert!(!encoded.contains("SECRET"));
        assert!(!encoded.contains(f.root.path().to_str().unwrap()));
    }
    assert!(client.cloud_resource("1", "999", &revision).await.is_err());
    assert!(client.cloud_resource("2", "1", &revision).await.is_err());
    assert!(
        client
            .cloud_directory_path("1", "1", &revision)
            .await
            .is_err()
    );
    f.api.rows.lock().unwrap().pop();
    client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    assert!(matches!(
        client.cloud_resource("1", "1", &revision).await,
        Err(Error::QueueChanged)
    ));
}
#[tokio::test]
async fn completed_cloud_download_is_associated_by_origin_and_rechecked_before_directory_access() {
    let f = Fixture::new(1);
    f.api.rows.lock().unwrap()[0]["fileSize"] = json!(MEDIA_BYTES.len());
    let service =
        DownloadService::with_backend(f.session.clone(), Arc::new(Files), 1024 * 1024).unwrap();
    let root = f.root.path().join("downloaded");
    std::fs::create_dir(&root).unwrap();
    let target = root.join("original.flac");
    let task = service
        .create_cloud(
            &json!(1),
            DownloadDestination::Path {
                path: target.clone(),
            },
            false,
        )
        .await
        .unwrap();
    let task = service
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    assert_eq!(task.status, DownloadStatus::Complete);
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    assert!(
        client
            .cloud_resource("1", "1", &revision)
            .await
            .unwrap()
            .local_available
    );
    assert_eq!(
        client
            .cloud_directory_path("1", "1", &revision)
            .await
            .unwrap()
            .canonicalize()
            .unwrap(),
        root.canonicalize().unwrap()
    );
    let mut changed = MEDIA_BYTES.to_vec();
    changed[0] ^= 1;
    std::fs::write(&target, changed).unwrap();
    assert!(
        client
            .cloud_directory_path("1", "1", &revision)
            .await
            .is_err()
    );
    std::fs::remove_file(&target).unwrap();
    assert!(
        !client
            .cloud_resource("1", "1", &revision)
            .await
            .unwrap()
            .local_available
    );
    #[cfg(unix)]
    {
        let other = f.root.path().join("other.flac");
        std::fs::write(&other, MEDIA_BYTES).unwrap();
        std::os::unix::fs::symlink(other, &target).unwrap();
        assert!(
            !client
                .cloud_resource("1", "1", &revision)
                .await
                .unwrap()
                .local_available
        );
        assert!(
            client
                .cloud_directory_path("1", "1", &revision)
                .await
                .is_err()
        );
    }
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(client.cloud_resource("1", "1", &revision).await.is_err());
}
#[tokio::test]
async fn ordinary_next_play_does_not_confuse_a_cloud_song_with_an_equal_numeric_catalog_id() {
    let f = Fixture::new(1);
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    client
        .play_selection(
            "1",
            PlaySelection::CloudBatch {
                query: None,
                ids: vec!["1".into()],
                revision: revision.clone(),
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let before = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(before.resource_kind, Some(PlayerResourceKind::Cloud));
    let added = client
        .player_action(
            "1",
            PlayerAction::NextPlay {
                id: "1".into(),
                quality: Quality::Standard,
                expected_queue_revision: before.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(before.play_id, added.play_id);
    assert_eq!(before.entry_id, added.entry_id);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(queue.tracks.len(), 2);
    assert_eq!(queue.tracks[1].track.name, "Catalog 1");
    let catalog = client.player_action("1", PlayerAction::Next).await.unwrap();
    assert_eq!(catalog.resource_kind, Some(PlayerResourceKind::Catalog));
    assert_eq!(catalog.track.unwrap().name, "Catalog 1");
    let again = client
        .player_action(
            "1",
            PlayerAction::NextPlay {
                id: "1".into(),
                quality: Quality::Standard,
                expected_queue_revision: catalog.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(catalog.queue_revision, again.queue_revision);
    client
        .player_action(
            "1",
            PlayerAction::QueueCloudBatch {
                query: None,
                ids: vec!["1".into()],
                revision,
                quality: Quality::Standard,
                expected_queue_revision: again.queue_revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 2);
}

#[tokio::test]
async fn row_selection_retains_its_captured_filter_and_order_while_batch_defaults_to_natural() {
    let f = Fixture::new(3);
    {
        let mut rows = f.api.rows.lock().unwrap();
        for (row, name) in rows.iter_mut().zip(["C", "A", "B"]) {
            row["songName"] = json!(name);
        }
    }
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let revision = page.revision.unwrap();
    let mut sorted = query(CloudTab::Uploaded);
    sorted.sort = CloudSort::Title;
    sorted.descending = false;
    sorted.offset = 200;
    client
        .play_selection(
            "1",
            PlaySelection::CloudBatch {
                ids: vec!["1".into(), "3".into()],
                revision: revision.clone(),
                quality: Quality::Standard,
                query: Some(sorted.clone()),
            },
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|t| t.track.id.as_str())
            .collect::<Vec<_>>(),
        ["3", "1"]
    );
    sorted.keyword = "no-such-cloud-song".into();
    assert!(
        client
            .play_selection(
                "1",
                PlaySelection::CloudBatch {
                    ids: vec!["1".into()],
                    revision: revision.clone(),
                    quality: Quality::Standard,
                    query: Some(sorted)
                }
            )
            .await
            .is_err()
    );
    client
        .play_selection(
            "1",
            PlaySelection::CloudBatch {
                ids: vec!["1".into(), "3".into()],
                revision,
                quality: Quality::Standard,
                query: None,
            },
        )
        .await
        .unwrap();
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|t| t.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "3"]
    );
}
