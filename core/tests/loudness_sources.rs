mod support;
use cumulus_core::{
    account::AccountSession,
    library::{AudioMetadata, ListOptions, LocalLibrary, LocalRevision, probe::AudioProbe},
    media::{
        ncm::{NcmWriteOptions, write_stream},
        tags::{track_plan, write_tagged_copy},
    },
    playback::source::SourceResolver,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::Path, sync::Arc};
struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> cumulus_core::Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("flac".into()),
            duration: 10.0,
            title: path.file_stem().unwrap().to_string_lossy().into(),
            album: String::new(),
            artists: vec![],
            bitrate: None,
            sample_rate: Some(48000),
            channels: Some(2),
            container: path.extension().unwrap().to_string_lossy().into(),
            cover_bytes: None,
        })
    }
}
#[tokio::test]
async fn tagged_files_and_ncm_use_current_container_metadata_while_cue_never_inherits_album_gain() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    fs::create_dir(&media).unwrap();
    for (extension, seed) in [
        (
            "mp3",
            include_bytes!("fixtures/tag_audio/seed.mp3").as_slice(),
        ),
        (
            "flac",
            include_bytes!("fixtures/tag_audio/seed.flac").as_slice(),
        ),
        (
            "m4a",
            include_bytes!("fixtures/tag_audio/seed.m4a").as_slice(),
        ),
    ] {
        let raw = root.path().join(format!("seed.{extension}"));
        fs::write(&raw, seed).unwrap();
        let plan = track_plan(
            &json!({"id":1,"name":"synthetic"}),
            &json!({"type":extension,"gain":-18}),
            false,
        )
        .unwrap();
        write_tagged_copy(&raw, media.join(format!("tagged.{extension}")), &plan, None).unwrap();
    }
    let metadata = json!({"musicId":1,"musicName":"ncm metadata","volumeDelta":-9,"format":"flac"});
    write_stream(
        &mut Cursor::new(include_bytes!("fixtures/tag_audio/seed.flac")),
        &mut fs::File::create(media.join("encoded.ncm")).unwrap(),
        NcmWriteOptions {
            metadata: Some(&metadata),
            cover: &[],
            audio_key: Some(b"synthetic-key-123"),
        },
    )
    .unwrap();
    fs::write(
        media.join("raw.flac"),
        include_bytes!("fixtures/tag_audio/seed.flac"),
    )
    .unwrap();
    fs::write(media.join("album.cue"), "FILE \"tagged.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Segment\"\n    INDEX 01 00:00:00\n").unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let library = Arc::new(LocalLibrary::with_probe(store.clone(), Arc::new(Probe)).unwrap());
    let scan = library.scan(&media).unwrap();
    assert!(scan.errors.is_empty(), "{:?}", scan.errors);
    let session = Arc::new(AccountSession::new(
        Arc::new(support::ScriptApi::new([])),
        Some(store),
    ));
    let resolver = SourceResolver::new(session.clone(), Some(library.clone()));
    let token = session.token().unwrap();
    for (name, expected) in [
        ("tagged.mp3", json!(-18)),
        ("tagged.flac", json!(-18)),
        ("tagged.m4a", json!(-18)),
        ("encoded.ncm", json!(-9)),
        ("raw.flac", Value::Null),
    ] {
        let source = resolver
            .resolve(
                &json!({"path":media.join(name),"volumeDelta":100,"gain":100}),
                &token,
            )
            .await
            .unwrap();
        assert_eq!(source.info["volume_delta"], expected, "{name}");
        assert_eq!(source.info["loudness_invalid"], false, "{name}");
    }
    let entries = library.list(ListOptions::default()).unwrap();
    let cue = entries
        .iter()
        .find(|item| matches!(item.revision, LocalRevision::Cue { .. }))
        .expect("indexed CUE");
    let resource = serde_json::to_value(library.resource(&cue.id).unwrap()).unwrap();
    let source = resolver.resolve(&resource, &token).await.unwrap();
    assert!(source.info["volume_delta"].is_null());
    assert_eq!(source.info["loudness_invalid"], false);
    let regular = entries
        .iter()
        .find(|item| item.path.ends_with("tagged.mp3"))
        .unwrap();
    let resource = serde_json::to_value(library.resource(&regular.id).unwrap()).unwrap();
    assert_eq!(
        resolver.resolve(&resource, &token).await.unwrap().info["volume_delta"],
        -18
    );
    fs::write(&regular.path, b"file replaced after scan").unwrap();
    assert!(resolver.resolve(&resource, &token).await.is_err());
}
