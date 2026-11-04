use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::SongDownloadState},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

struct NoNetwork;
impl Api for NoNetwork {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("playback download status must use local records only") })
    }
}

#[tokio::test]
async fn badge_reads_only_matching_account_song_tasks_and_requires_a_present_file() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    let epoch = client.account().unwrap().epoch;
    let put = |owner: &str, task: &str, record: Value| {
        store
            .put(owner, "download_tasks_v1", task, &record)
            .unwrap();
    };
    let read = || client.song_download_state(&epoch, "123");
    let path = root.path().join("published.wav");
    put(
        "2",
        "other-account",
        json!({"song_id":"123","status":"downloading"}),
    );
    put(
        "1",
        "cloud-id",
        json!({"song_id":"123","origin":{"kind":"cloud"},"status":"downloading"}),
    );
    put(
        "1",
        "other-song",
        json!({"song_id":"124","status":"downloading"}),
    );
    put(
        "1",
        "mv-id",
        json!({"song_id":"123","origin":{"kind":"mv"},"status":"downloading"}),
    );
    put(
        "1",
        "cancelled",
        json!({"song_id":"123","status":"cancelled"}),
    );
    put(
        "1",
        "published",
        json!({"song_id":"123","status":"complete","target":path}),
    );
    assert_eq!(read().await.unwrap(), SongDownloadState::None);
    std::fs::write(&path, b"synthetic existing file").unwrap();
    assert_eq!(read().await.unwrap(), SongDownloadState::Complete);
    put(
        "1",
        "another-quality",
        json!({"song_id":123,"origin":{"kind":"song"},"status":"queued"}),
    );
    assert_eq!(read().await.unwrap(), SongDownloadState::Waiting);
    put(
        "1",
        "another-quality",
        json!({"song_id":123,"status":"processing"}),
    );
    assert_eq!(read().await.unwrap(), SongDownloadState::Working);
    put(
        "1",
        "another-quality",
        json!({"song_id":123,"status":"error"}),
    );
    assert_eq!(read().await.unwrap(), SongDownloadState::Complete);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(read().await.unwrap(), SongDownloadState::Failed);
    session
        .adopt(api, json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(read().await, Err(Error::StaleResponse)));
    let next = client.account().unwrap().epoch;
    assert_eq!(
        client.song_download_state(&next, "123").await.unwrap(),
        SongDownloadState::Working
    );
}

#[tokio::test]
async fn voice_badge_is_keyed_by_episode_instead_of_the_backing_song() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let api = Arc::new(NoNetwork);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    let epoch = client.account().unwrap().epoch;
    let put = |owner: &str, task: &str, record: Value| {
        store
            .put(owner, "download_tasks_v1", task, &record)
            .unwrap();
    };
    let read = || client.voice_download_state(&epoch, "401");
    put(
        "1",
        "collision",
        json!({"song_id":"401","status":"downloading"}),
    );
    put(
        "1",
        "backing-collision",
        json!({"song_id":"401","origin":{"kind":"voice","voice_id":"402"},"status":"downloading"}),
    );
    put(
        "2",
        "other-account",
        json!({"song_id":"1","origin":{"kind":"voice","voice_id":"401"},"status":"downloading"}),
    );
    assert_eq!(read().await.unwrap(), SongDownloadState::None);
    let path = root.path().join("voice.wav");
    let voice = |status: &str| json!({"song_id":"1","origin":{"kind":"voice","voice_id":"401"},"status":status,"target":path});
    put("1", "selected", voice("queued"));
    assert_eq!(read().await.unwrap(), SongDownloadState::Waiting);
    put("1", "selected", voice("downloading"));
    assert_eq!(read().await.unwrap(), SongDownloadState::Working);
    assert_eq!(
        client.song_download_state(&epoch, "1").await.unwrap(),
        SongDownloadState::None
    );
    put("1", "selected", voice("complete"));
    assert_eq!(read().await.unwrap(), SongDownloadState::None);
    std::fs::write(path, b"synthetic present file").unwrap();
    assert_eq!(read().await.unwrap(), SongDownloadState::Complete);
    session
        .adopt(api, json!({"userId":2}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(read().await, Err(Error::StaleResponse)));
}
