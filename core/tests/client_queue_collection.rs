#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod support;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::PlayerAction},
    library::{AudioMetadata, ListOptions, LocalLibrary, LocalRevision, probe::AudioProbe},
    playback::{PlaybackMode, PlaybackQueue},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move { panic!("queue collection attempted HTTP: {path}") })
    }
}
struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            title: path.file_stem().unwrap().to_string_lossy().into_owned(),
            album: "Local album".into(),
            artists: vec!["Local artist".into()],
            duration: 120.0,
            format: Some("wav".into()),
            container: "wav".into(),
            bitrate: None,
            sample_rate: Some(8000),
            channels: Some(1),
            cover_bytes: None,
        })
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    store: Arc<StateStore>,
    session: Arc<AccountSession>,
    client: MusicClient,
    library: LocalLibrary,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(&dir.path().join("state"), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let session = Arc::new(AccountSession::new(Arc::new(Offline), Some(store.clone())));
        session
            .adopt(
                Arc::new(Offline),
                json!({"userId":9}),
                &session.token().unwrap(),
            )
            .unwrap();
        let library = LocalLibrary::with_probe(store.clone(), Arc::new(Probe)).unwrap();
        let client = MusicClient::with_session(session.clone(), registry)
            .with_audio_factory(|| Ok(support::audio_engine(Arc::new(support::Device::default()))));
        Self {
            dir,
            store,
            session,
            client,
            library,
        }
    }
    fn queue(&self, owner: &str, resources: Vec<Value>) {
        PlaybackQueue::new(Some(self.store.clone()), owner)
            .unwrap()
            .replace(resources, 0, json!({"kind":"songs"}))
            .unwrap();
    }
    fn scan(&self, count: usize, cue: bool) -> Vec<cumulus_core::library::LibraryItem> {
        let media = self.dir.path().join("PRIVATE-media");
        std::fs::create_dir(&media).unwrap();
        for i in 0..count {
            std::fs::write(media.join(format!("local-{i}.wav")), [1, 2, 3, 4]).unwrap();
        }
        if cue {
            std::fs::write(media.join("segments.cue"),
            "FILE \"local-0.wav\" WAVE\n TRACK 01 AUDIO\n TITLE \"First segment\"\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n TITLE \"Second segment\"\n INDEX 01 00:30:00\n").unwrap();
        }
        self.library.scan(&media).unwrap();
        self.library
            .list(ListOptions {
                include_containers: true,
                ..Default::default()
            })
            .unwrap()
    }
    fn resource(&self, id: &str) -> Value {
        serde_json::to_value(self.library.resource(id).unwrap()).unwrap()
    }
}
fn online(id: u64) -> Value {
    json!({"id":id.to_string(),"song":{"id":id,"name":format!("Song {id}"),"ar":[{"name":"Artist"}],"cookie":"PRIVATE","url":"https://private.invalid/PRIVATE"},"authorization":"PRIVATE"})
}

#[tokio::test]
async fn mixed_queue_collects_songs_downloads_matched_files_and_cue_without_network_or_audio() {
    let f = Fixture::new();
    let items = f.scan(2, true);
    let file = items
        .iter()
        .find(|i| {
            matches!(i.revision, LocalRevision::File { .. }) && i.path.ends_with("local-0.wav")
        })
        .unwrap();
    let unlinked = items
        .iter()
        .find(|i| {
            matches!(i.revision, LocalRevision::File { .. }) && i.path.ends_with("local-1.wav")
        })
        .unwrap();
    let cue = items
        .iter()
        .find(|i| matches!(i.revision, LocalRevision::Cue { track: 1, .. }))
        .unwrap();
    f.library
        .map_online(&f.session, &file.id, &json!(3))
        .unwrap();
    f.library
        .map_online(&f.session, &cue.id, &json!(4))
        .unwrap();
    let mut download = online(2);
    download["download_task_id"] = json!("saved");
    download["path"] = json!("PRIVATE");
    let mut voice = online(900);
    voice["voice"] = json!({"radioId":7});
    voice["song"]["voice_id"] = json!(99);
    let mut cloud = online(5);
    cloud["cloud_song_id"] = json!(5);
    f.queue(
        "9",
        vec![
            online(2),
            download,
            voice,
            f.resource(&file.id),
            f.resource(&unlinked.id),
            f.resource(&cue.id),
            cloud,
            online(1),
        ],
    );
    let before = f.client.player_queue("1").await.unwrap();
    let result = f
        .client
        .player_queue_collection("1", &before.revision)
        .await
        .unwrap();
    assert_eq!(
        (result.total, result.unsupported, result.duplicates),
        (8, 2, 1)
    );
    assert_eq!(
        result
            .tracks
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["2", "3", "4", "5", "1"]
    );
    assert!(
        result
            .tracks
            .iter()
            .all(|t| !t.local && t.voice_id.is_none())
    );
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("PRIVATE") && !serialized.contains("local_revision"));
    let after = f.client.player_queue("1").await.unwrap();
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.current, after.current);
    let native = f
        .client
        .subscribe_playback("1")
        .await
        .unwrap()
        .current()
        .unwrap();
    assert!(native.play_id.is_none());
}

#[tokio::test]
async fn changed_file_revision_and_account_do_not_inherit_an_old_local_song_link() {
    let f = Fixture::new();
    let item = f.scan(1, false).pop().unwrap();
    f.library
        .map_online(&f.session, &item.id, &json!(3))
        .unwrap();
    let old = f.resource(&item.id);
    f.queue("9", vec![old.clone()]);
    std::fs::write(&item.path, [5, 6, 7, 8, 9, 10]).unwrap();
    f.library.scan(item.path.parent().unwrap()).unwrap();
    f.library
        .map_online(&f.session, &item.id, &json!(99))
        .unwrap();
    let current = f.client.player_queue("1").await.unwrap();
    let stale_file = f
        .client
        .player_queue_collection("1", &current.revision)
        .await
        .unwrap();
    assert!(stale_file.tracks.is_empty());
    assert_eq!(stale_file.unsupported, 1);
    f.client
        .player_action(
            "1",
            PlayerAction::Mode {
                mode: PlaybackMode::Random,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.client
            .player_queue_collection("1", &current.revision)
            .await,
        Err(Error::QueueChanged)
    ));
    f.session
        .adopt(
            Arc::new(Offline),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    f.queue("10", vec![f.resource(&item.id)]);
    assert!(matches!(
        f.client.player_queue_collection("1", "0").await,
        Err(Error::StaleResponse)
    ));
    let other = f.client.player_queue("2").await.unwrap();
    let selection = f
        .client
        .player_queue_collection("2", &other.revision)
        .await
        .unwrap();
    assert!(selection.tracks.is_empty());
    assert_eq!(selection.unsupported, 1);
}

#[tokio::test]
async fn full_local_collection_is_not_limited_to_the_visible_500_row_mapping_page() {
    let f = Fixture::new();
    let items = f.scan(501, false);
    for (item, id) in [(&items[0], 7), (&items[500], 8)] {
        f.library
            .map_online(&f.session, &item.id, &json!(id))
            .unwrap();
    }
    f.queue("9", items.iter().map(|i| f.resource(&i.id)).collect());
    let before = f.client.player_queue("1").await.unwrap();
    let selection = f
        .client
        .player_queue_collection("1", &before.revision)
        .await
        .unwrap();
    assert_eq!(
        (selection.total, selection.unsupported, selection.duplicates),
        (501, 499, 0)
    );
    assert_eq!(
        selection
            .tracks
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["7", "8"]
    );
}
