use super::*;
use cumulus_core::{playback::controller::PlaybackStatus, storage::StateStore};
use std::sync::atomic::{AtomicU8, Ordering};

struct Network {
    upstream: Server,
    state: AtomicU8,
    calls: Mutex<Vec<String>>,
}
impl Network {
    fn new() -> Self {
        let upstream = Server::new();
        *upstream.detail.lock().unwrap() = json!({
            "id":73, "name":"Cached list", "userId":9, "privacy":10, "specialType":0,
            "trackIds":[{"id":11,"v":0},{"id":77,"v":-1},{"id":42,"v":0}],
            "description":"Synthetic text",
            "coverImgUrl":"https://p1.music.126.net/list.jpg?private=test",
            "creator":{"userId":9,"nickname":"Synthetic","avatarUrl":"https://p1.music.126.net/avatar.jpg?private=test"},
            "tags":[], "updateTime":2020010100000u64,
        });
        Self {
            upstream,
            state: AtomicU8::new(0),
            calls: Mutex::default(),
        }
    }
    fn set(&self, state: u8) {
        self.state.store(state, Ordering::Release);
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Api for Network {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.to_owned());
            match self.state.load(Ordering::Acquire) {
                1 => Err(Error::Transport("synthetic offline".into())),
                2 => Ok(json!({"code":403, "msg":"synthetic permission changed"})),
                3 if path == "/api/v3/song/detail" => {
                    Err(Error::Transport("synthetic hydration interrupted".into()))
                }
                4 => Ok(json!({"code":301, "msg":"synthetic expired"})),
                5 => Err(Error::HttpStatus {
                    status: 503,
                    endpoint: Some(path.to_owned()),
                    body: Some(Box::new(json!({"code":"403"}))),
                }),
                _ => {
                    let mut result = self.upstream.request(path, payload, cache).await?;
                    if path == "/api/v3/song/detail" {
                        for row in result["songs"].as_array_mut().unwrap() {
                            row["al"]["picUrl"] =
                                json!("https://p1.music.126.net/album.jpg?private=test");
                            row["authUrl"] = json!("https://music.126.net/source?private=test");
                            row["path"] = json!("/private/source/fixture.wav");
                        }
                    }
                    Ok(result)
                }
            }
        })
    }
}

fn request(offset: usize, revision: Option<String>) -> PageRequest {
    PageRequest {
        offset,
        limit: 2,
        revision,
    }
}

fn read_record(profile: &Path) -> Option<Value> {
    ProfileRegistry::open(profile, ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playlist-offline-snapshot-v1", "73")
        .unwrap()
}

#[tokio::test]
async fn complete_owned_snapshot_reopens_offline_and_plays_only_verified_indexed_cue() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let wav = media.join("fixture.wav");
    std::fs::write(&wav, b"synthetic cue container").unwrap();
    std::fs::write(
        media.join("fixture.cue"),
        concat!(
            "FILE \"fixture.wav\" WAVE\n",
            "TRACK 01 AUDIO\nTITLE \"Fixture one\"\nINDEX 01 00:00:00\n",
            "TRACK 02 AUDIO\nTITLE \"Fixture two\"\nINDEX 01 00:05:00\n",
        ),
    )
    .unwrap();
    let network = Arc::new(Network::new());
    let (reference, raw_revision, mixed_revision, cached_at) = {
        let (client, session, library) = open(&profile, network.clone(), Some(9));
        assert_eq!(library.scan(&media).unwrap().cue_updated, 1);
        let indexed = client.local_page(LocalQuery::default()).await.unwrap();
        let chosen = indexed
            .tracks
            .into_iter()
            .find(|row| row.track.name == "Fixture two")
            .unwrap();
        let reference = PlaylistLocalRef {
            id: chosen.track.id,
            revision: chosen.revision,
        };
        let revision = project(&session, &network.upstream).revision;
        client
            .mutate_playlist_local("1", "73", &revision, add(std::slice::from_ref(&reference)))
            .await
            .unwrap();
        let first = client
            .playlist_page_at("73", request(0, None), None)
            .await
            .unwrap()
            .value;
        assert!(first.more && first.playlist_offline.is_none());
        assert!(read_record(&profile).is_none());
        network.set(1);
        assert!(matches!(
            client.playlist_page_at("73", request(0, None), None).await,
            Err(Error::Transport(_))
        ));
        network.set(0);
        let mixed_revision = first.playlist_local.unwrap().revision;
        client
            .playlist_page_at(
                "73",
                request(2, first.revision.clone()),
                Some(mixed_revision.clone()),
            )
            .await
            .unwrap();
        let record = read_record(&profile).unwrap();
        let storage = record.to_string();
        assert!(
            !storage.contains("https:")
                && !storage.contains("private=test")
                && !storage.contains("fixture.wav")
        );
        assert_eq!(
            record["rows"][0],
            json!({"kind":"local","id":reference.id,"revision":reference.revision})
        );
        assert_eq!(record["rows"][1]["id"], "11");
        let cached_at = record["cached_at_ms"].as_u64().unwrap();
        (reference, first.revision, mixed_revision, cached_at)
    };

    network.set(1);
    let (client, _, _) = open(&profile, network.clone(), Some(9));
    let first = client
        .playlist_page_at("73", request(0, None), None)
        .await
        .unwrap()
        .value;
    assert_eq!(first.playlist_offline.unwrap().cached_at_ms, cached_at);
    assert_eq!(first.revision, raw_revision);
    assert_eq!(first.playlist_local.unwrap().revision, mixed_revision);
    assert!(first.tracks[0].local && !first.tracks[0].missing);
    assert_eq!(first.tracks[1].name, "Online 11");
    assert!(first.tracks[1].missing && !first.tracks[1].local);
    assert!(matches!(
        client
            .playlist_page_at("73", request(2, raw_revision.clone()), Some("other".into()))
            .await,
        Err(Error::QueueChanged)
    ));
    let second = client
        .playlist_page_at(
            "73",
            request(2, raw_revision.clone()),
            Some(mixed_revision.clone()),
        )
        .await
        .unwrap()
        .value;
    assert_eq!(second.playlist_offline.unwrap().cached_at_ms, cached_at);
    assert_eq!(
        second
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        ["77", "42"]
    );
    assert!(second.tracks.iter().all(|track| track.missing));

    let before = network.calls();
    client
        .play_selection(
            "1",
            PlaySelection::OfflinePlaylist {
                id: "73".into(),
                revision: mixed_revision.clone(),
                start_id: Some(reference.id.clone()),
                double_click: false,
            },
        )
        .await
        .unwrap();
    let playing = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            if view.state == PlaybackStatus::Playing {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(playing.resource_kind, Some(PlayerResourceKind::Local));
    assert_eq!(playing.track.unwrap().id, reference.id);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.id.as_str())
            .collect::<Vec<_>>(),
        [reference.id]
    );
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::OfflinePlaylist {
                    id: "73".into(),
                    revision: mixed_revision,
                    start_id: Some("11".into()),
                    double_click: false,
                }
            )
            .await,
        Err(Error::State("cached playlist song is unavailable"))
    ));
    assert_eq!(
        network.calls(),
        before,
        "offline playback must not call service"
    );
    assert_eq!(std::fs::read(wav).unwrap(), b"synthetic cue container");
}

#[tokio::test]
async fn owned_cache_isolated_from_denials_other_accounts_and_changed_membership() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let network = Arc::new(Network::new());
    let (client, _, _) = open(&profile, network.clone(), Some(9));
    let whole = || PageRequest {
        offset: 0,
        limit: 10,
        revision: None,
    };
    let first = client
        .playlist_page_at("73", whole(), None)
        .await
        .unwrap()
        .value;
    let revision = first.playlist_local.unwrap().revision;
    assert!(read_record(&profile).is_some());

    network.set(3);
    let from_hydration = client
        .playlist_page_at("73", whole(), None)
        .await
        .unwrap()
        .value;
    assert!(from_hydration.playlist_offline.is_some());
    let (other, _, _) = open(&profile, network.clone(), Some(8));
    network.set(1);
    assert!(matches!(
        other.playlist_page_at("73", whole(), None).await,
        Err(Error::Transport(_))
    ));
    let (guest, _, _) = open(&profile, network.clone(), None);
    let before_guest = network.calls();
    assert!(
        guest
            .play_selection(
                "0",
                PlaySelection::OfflinePlaylist {
                    id: "73".into(),
                    revision: revision.clone(),
                    start_id: None,
                    double_click: false,
                }
            )
            .await
            .is_err()
    );
    assert_eq!(network.calls(), before_guest);

    network.set(5);
    assert!(matches!(client.playlist_page_at("73", whole(), None).await,
        Err(error) if error.is_service_code(403)));

    network.set(0);
    client.playlist_page_at("73", whole(), None).await.unwrap();
    network.set(2);
    assert!(matches!(
        client.playlist_page_at("73", whole(), None).await,
        Err(error) if error.is_service_code(403)
    ));
    assert!(read_record(&profile).is_none());
    network.set(1);
    assert!(matches!(
        client.playlist_page_at("73", whole(), None).await,
        Err(Error::Transport(_))
    ));

    network.set(0);
    client.playlist_page_at("73", whole(), None).await.unwrap();
    let (expiring, _, _) = open(&profile, network.clone(), Some(9));
    network.set(4);
    assert!(
        matches!(expiring.playlist_page_at("73", whole(), None).await,
        Err(error) if error.is_service_code(301))
    );
    network.set(1);
    assert!(
        expiring
            .playlist_page_at("73", whole(), None)
            .await
            .is_err()
    );
    let store: std::sync::Arc<StateStore> = ProfileRegistry::open(&profile, ApiOrigin::Pc)
        .unwrap()
        .store();
    store
        .put(
            "9",
            "playlist-local-membership-v1",
            "73",
            &json!({"generation":1, "local":[]}),
        )
        .unwrap();
    network.set(1);
    assert!(matches!(
        client.playlist_page_at("73", whole(), None).await,
        Err(Error::Transport(_))
    ));
    assert!(matches!(
        client
            .play_selection(
                "1",
                PlaySelection::OfflinePlaylist {
                    id: "73".into(),
                    revision,
                    start_id: None,
                    double_click: false,
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
}
