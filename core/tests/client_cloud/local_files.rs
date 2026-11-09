use super::*;
use cumulus_core::library::{AudioMetadata, ListOptions, LocalLibrary, probe::AudioProbe};
use std::path::Path;
pub(super) struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> cumulus_core::Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("flac".into()),
            duration: 120.0,
            title: "Mapped file".into(),
            album: "Local album".into(),
            artists: vec!["Local artist".into()],
            bitrate: None,
            sample_rate: Some(44100),
            channels: Some(2),
            container: "flac".into(),
            cover_bytes: None,
        })
    }
}
pub(super) fn library(f: &Fixture) -> LocalLibrary {
    LocalLibrary::with_probe(
        ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
            .unwrap()
            .store(),
        Arc::new(Probe),
    )
    .unwrap()
}
async fn resource(client: &MusicClient, epoch: &str) -> CloudResourceView {
    let page = client
        .cloud_page(epoch, query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    client
        .cloud_resource(epoch, "1", &page.revision.unwrap())
        .await
        .unwrap()
}
#[tokio::test]
async fn scanned_files_require_explicit_catalog_mapping_current_revision_and_account_even_for_unshareable_cloud_songs()
 {
    let f = Fixture::new(1);
    f.api.rows.lock().unwrap()[0]["simpleSong"] = json!({"id":9001,"name":"Private cloud track","t":1,"privilege":{"st":-100},"ar":[],"al":{},"dt":120000});
    let media = tempfile::tempdir().unwrap();
    let path = media.path().join("file-1.flac");
    std::fs::write(&path, resources::MEDIA_BYTES).unwrap();
    let lib = library(&f);
    lib.scan(media.path()).unwrap();
    let id = lib.list(ListOptions::default()).unwrap()[0].id.clone();
    let client = f.client().with_local_probe(Arc::new(Probe));
    assert!(
        !resource(&client, "1").await.local_available,
        "same filename is not an association"
    );
    lib.map_online(&f.session, &id, &json!(1)).unwrap();
    assert!(
        !resource(&client, "1").await.local_available,
        "cloud ID must not masquerade as a catalog ID"
    );
    lib.map_online(&f.session, &id, &json!(9001)).unwrap();
    let r = resource(&client, "1").await;
    assert!(r.local_available);
    assert!(r.track.public_song_id.is_none());
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains(media.path().to_str().unwrap()));
    assert!(!json.contains("_cloud_catalog_id"));
    let reveal = client
        .cloud_file_reveal("1", "1", &r.revision)
        .await
        .unwrap();
    assert_eq!(reveal.path(), path.canonicalize().unwrap());
    reveal.check_current().unwrap();
    lib.map_online(&f.session, &id, &json!(9002)).unwrap();
    assert!(reveal.check_current().is_err());
    lib.map_online(&f.session, &id, &json!(9001)).unwrap();
    let reveal = client
        .cloud_file_reveal("1", "1", &r.revision)
        .await
        .unwrap();
    std::fs::write(&path, b"changed source bytes").unwrap();
    assert!(reveal.check_current().is_err());
    assert!(!resource(&client, "1").await.local_available);
    assert!(
        client
            .cloud_file_reveal("1", "1", &r.revision)
            .await
            .is_err()
    );
    lib.scan(media.path()).unwrap();
    assert!(
        !resource(&client, "1").await.local_available,
        "rescan cannot inherit a mapping for old bytes"
    );
    lib.map_online(&f.session, &id, &json!(9001)).unwrap();
    assert!(resource(&client, "1").await.local_available);
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(client.cloud_resource("1", "1", &r.revision).await.is_err());
    let epoch = f.session.token().unwrap().generation.to_string();
    assert!(!resource(&client, &epoch).await.local_available);
}
#[tokio::test]
async fn cue_reveal_selects_the_audio_only_while_its_sheet_and_source_revisions_match() {
    let f = Fixture::new(1);
    let media = tempfile::tempdir().unwrap();
    let path = media.path().join("audio.flac");
    std::fs::write(&path, resources::MEDIA_BYTES).unwrap();
    let cue = media.path().join("album.cue");
    std::fs::write(&cue,"FILE \"audio.flac\" WAVE\n TRACK 01 AUDIO\n TITLE \"First\"\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n INDEX 01 00:01:00\n").unwrap();
    let lib = library(&f);
    lib.scan(media.path()).unwrap();
    let item = lib
        .list(ListOptions::default())
        .unwrap()
        .into_iter()
        .find(|item| {
            matches!(
                item.revision,
                cumulus_core::library::LocalRevision::Cue { .. }
            )
        })
        .unwrap();
    lib.map_online(&f.session, &item.id, &json!(999999))
        .unwrap();
    let client = f.client().with_local_probe(Arc::new(Probe));
    let r = resource(&client, "1").await;
    assert!(r.local_available);
    let reveal = client
        .cloud_file_reveal("1", "1", &r.revision)
        .await
        .unwrap();
    assert_eq!(reveal.path(), path.canonicalize().unwrap());
    reveal.check_current().unwrap();
    assert_ne!(reveal.path(), cue.canonicalize().unwrap());
    std::fs::write(&cue, "REM changed sheet\n").unwrap();
    assert!(reveal.check_current().is_err());
    assert!(!resource(&client, "1").await.local_available);
    assert!(
        client
            .cloud_file_reveal("1", "1", &r.revision)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn corrupt_local_mapping_degrades_only_the_file_action_and_cannot_disclose_private_state() {
    let f = Fixture::new(1);
    let store = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    store
        .put(
            "1",
            "local_map_v1",
            "synthetic",
            &json!("SECRET/path/credential"),
        )
        .unwrap();
    let client = f.client().with_local_probe(Arc::new(Probe));
    let r = resource(&client, "1").await;
    assert!(!r.local_available);
    assert!(r.local_issue.is_some());
    assert_eq!(r.track.public_song_id, Some("999999".into()));
    assert!(!serde_json::to_string(&r).unwrap().contains("SECRET"));
    assert!(
        client
            .cloud_file_reveal("1", "1", &r.revision)
            .await
            .is_err()
    );
    client
        .play_selection(
            "1",
            PlaySelection::Cloud {
                query: query(CloudTab::Uploaded),
                revision: r.revision,
                start_id: Some("1".into()),
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        client.player_snapshot("1").await.unwrap().resource_kind,
        Some(PlayerResourceKind::Cloud)
    );
}
