use super::*;
use cumulus_core::{playback::controller::PlaybackStatus, storage::StateStore};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

struct Network {
    upstream: Server,
    state: AtomicU8,
    deleted: AtomicBool,
    defer_delete_readback: AtomicBool,
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
            deleted: AtomicBool::new(false),
            defer_delete_readback: AtomicBool::new(false),
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
            if path == "/api/playlist/delete" {
                assert_eq!(payload, json!({"pid":"73"}));
                assert!(!self.deleted.swap(true, Ordering::AcqRel));
                return Ok(json!({"code":200}));
            }
            if path == "/api/user/playlist" {
                if self.defer_delete_readback.load(Ordering::Acquire) {
                    return Err(Error::Transport("readback interrupted".into()));
                }
                let playlists = if self.deleted.load(Ordering::Acquire) {
                    Vec::new()
                } else {
                    vec![self.upstream.detail.lock().unwrap().clone()]
                };
                return Ok(json!({"code":200, "playlist":playlists, "more":false}));
            }
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
                _ if path == "/api/music-vip-membership/client/vip/info" => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    Ok(json!({"code":200,"data":{"userId":"9","now":now,
                        "associator":{"expireTime":now+300_000,"vipLevel":7}}}))
                }
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
                start_local: true,
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
                    start_local: true,
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
                    start_local: true,
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
                    start_local: true,
                    double_click: false,
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn confirmed_delete_tombstones_cached_playlist_in_direct_and_reconciled_paths() {
    for deferred in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("profile");
        let network = Arc::new(Network::new());
        let (client, _, _) = open(&profile, network.clone(), Some(9));
        let whole = || PageRequest {
            offset: 0,
            limit: 10,
            revision: None,
        };
        client.playlist_page_at("73", whole(), None).await.unwrap();
        assert!(read_record(&profile).is_some());
        let editor = client.playlist_editor("1", "73").await.unwrap().value;
        network
            .defer_delete_readback
            .store(deferred, Ordering::Release);
        let deletion = client
            .mutate_playlist(
                "1",
                PlaylistCommand::Delete {
                    id: "73".into(),
                    revision: editor.revision,
                },
            )
            .await;
        let operation = if deferred {
            assert!(matches!(deletion, Err(Error::Transport(_))));
            assert!(
                read_record(&profile).is_some(),
                "uncertain write is not deletion proof"
            );
            let operations = client.playlist_operations("1").unwrap().value;
            assert_eq!(operations.len(), 1);
            let op = operations.into_iter().next().unwrap().operation_id;
            network
                .defer_delete_readback
                .store(false, Ordering::Release);
            client.reconcile_playlist("1", &op).await.unwrap().value
        } else {
            deletion.unwrap().value
        };
        assert_eq!(
            operation.phase,
            cumulus_core::playlist::PlaylistPhase::Confirmed
        );
        let registry = ProfileRegistry::open(&profile, ApiOrigin::Pc).unwrap();
        let store = registry.store();
        assert_eq!(
            store.get("9", "playlist-offline-deleted-v1", "73").unwrap(),
            Some(json!(true))
        );
        assert!(read_record(&profile).is_none());
        assert_eq!(
            store
                .get("9", "playlist-mutations", &operation.operation_id)
                .unwrap()
                .unwrap()["phase"],
            "confirmed"
        );

        // Simulate a stale authoritative detail already in flight at the
        // time of confirmation: no cached row can be republished.
        assert!(
            client
                .playlist_page_at("73", whole(), None)
                .await
                .unwrap()
                .value
                .playlist_offline
                .is_none()
        );
        assert!(read_record(&profile).is_none());
        network.set(1);
        let (reopened, _, _) = open(&profile, network.clone(), Some(9));
        assert!(matches!(
            reopened.playlist_page_at("73", whole(), None).await,
            Err(Error::Transport(_))
        ));
        assert_eq!(
            network
                .calls()
                .iter()
                .filter(|route| *route == "/api/playlist/delete")
                .count(),
            1
        );
    }
}

const PUBLISHED_FLAC: &[u8] = include_bytes!("../fixtures/tag_audio/seed.flac");

fn publish_fixture_song(
    profile: &Path,
    session: &Arc<AccountSession>,
    path: &Path,
    song_id: &str,
    encrypted: bool,
) -> String {
    use cumulus_core::{
        download::{
            offline::{OfflineLibrary, PublishedDownload},
            service::{DownloadDestination, DownloadOptions, DownloadService, DownloadStatus},
        },
        media::{
            integrity::inspect_file,
            ncm::{NcmWriteOptions, write_file},
        },
        membership::Quality,
    };

    let token = session.token().unwrap();
    let uid = token.uid.as_deref().unwrap();
    let registry = ProfileRegistry::open(profile, ApiOrigin::Pc).unwrap();
    let mut task = DownloadService::new(session.clone())
        .unwrap()
        .create(
            &json!(song_id),
            DownloadDestination::Path {
                path: path.to_path_buf(),
            },
            DownloadOptions::default(),
        )
        .unwrap();
    if encrypted {
        let source = path.with_extension("source.flac");
        std::fs::write(&source, PUBLISHED_FLAC).unwrap();
        write_file(source, path, NcmWriteOptions::default()).unwrap();
    } else {
        std::fs::write(path, PUBLISHED_FLAC).unwrap();
    }
    let integrity = inspect_file(path).unwrap();
    let song = json!({"id":song_id,"name":"Synthetic publication","dt":1000,
        "privilege":{"fee":1,"flag":4}});
    let resource = json!({"id":song_id,"level":"lossless","fee":1,"flag":4,"payed":0});
    let record = OfflineLibrary::new(session.clone())
        .unwrap()
        .register(
            &token,
            &PublishedDownload {
                task_id: task.id.clone(),
                song_id: song_id.into(),
                requested_quality: Quality::Lossless,
                target: path.to_path_buf(),
                song: song.clone(),
                resource: resource.clone(),
                encrypted,
                integrity: integrity.clone(),
            },
        )
        .unwrap();
    task.status = DownloadStatus::Complete;
    task.target = Some(record.path);
    task.output_integrity = Some(integrity);
    task.song = Some(song);
    task.resource = Some(resource);
    task.encrypted = encrypted;
    registry
        .store()
        .put(
            uid,
            "download_tasks_v1",
            &task.id,
            &serde_json::to_value(&task).unwrap(),
        )
        .unwrap();
    task.id
}

fn cached_whole() -> PageRequest {
    PageRequest {
        offset: 0,
        limit: 10,
        revision: None,
    }
}

#[tokio::test]
async fn cached_catalog_row_plays_exact_same_account_publication_without_a_url() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let network = Arc::new(Network::new());
    let device = Arc::new(playback_support::Device::default());
    device
        .pinned_search_supported
        .store(true, Ordering::Release);
    let native = device.clone();
    let (client, session, library) = open(&profile, network.clone(), Some(9));
    let client =
        client.with_audio_factory(move || Ok(playback_support::audio_engine(native.clone())));
    let plain = temp.path().join("download11.flac");
    let task = publish_fixture_song(&profile, &session, &plain, "11", false);
    let encrypted_path = temp.path().join("locked42.ncm");
    let encrypted = publish_fixture_song(&profile, &session, &encrypted_path, "42", true);
    let media = temp.path().join("local");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("fixture.wav"), b"synthetic local member").unwrap();
    library.scan(&media).unwrap();
    let local = refs(&client).await.remove(0);
    let previous = project(&session, &network.upstream);
    client
        .mutate_playlist_local(
            "1",
            "73",
            &previous.revision,
            add(std::slice::from_ref(&local)),
        )
        .await
        .unwrap();
    client
        .playlist_page_at("73", cached_whole(), None)
        .await
        .unwrap();
    network.set(1);
    let page = client
        .playlist_page_at("73", cached_whole(), None)
        .await
        .unwrap()
        .value;
    assert_eq!(page.playlist_offline.unwrap().download_ids, ["11"]);
    assert_eq!(
        page.tracks
            .iter()
            .map(|r| (&*r.id, r.local, r.missing))
            .collect::<Vec<_>>(),
        [
            (local.id.as_str(), true, false),
            ("11", false, false),
            ("77", true, true),
            ("42", false, true)
        ]
    );
    let revision = page.playlist_local.unwrap().revision;
    let before = network.calls();
    let play = |id: &str, local, double_click| PlaySelection::OfflinePlaylist {
        id: "73".into(),
        revision: revision.clone(),
        start_id: Some(id.into()),
        start_local: local,
        double_click,
    };
    assert!(
        client
            .play_selection("1", play("11", true, false))
            .await
            .is_err()
    );
    assert!(
        client
            .play_selection("1", play("42", false, false))
            .await
            .is_err()
    );
    client
        .play_selection("1", play("11", false, false))
        .await
        .unwrap();
    let playing = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            if view.state == PlaybackStatus::Playing {
                break view;
            }
            assert_ne!(view.state, PlaybackStatus::Error);
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(playing.resource_kind, Some(PlayerResourceKind::Downloaded));
    assert_eq!(playing.download_task_id.as_deref(), Some(task.as_str()));
    assert_eq!(
        device.state.lock().unwrap().last_pinned.as_ref(),
        Some(&(false, PUBLISHED_FLAC.to_vec()))
    );
    let paused = client
        .play_selection("1", play("11", false, true))
        .await
        .unwrap();
    assert_eq!(paused.play_id, playing.play_id);
    assert_eq!(paused.state, PlaybackStatus::Paused);
    let store = ProfileRegistry::open(&profile, ApiOrigin::Pc)
        .unwrap()
        .store();
    let queue = store.get("9", "playback", "queue").unwrap().unwrap();
    assert_eq!(queue["items"].as_array().unwrap().len(), 2);
    assert_eq!(queue["items"][0]["resource"]["local_id"], local.id);
    let row = &queue["items"][1]["resource"];
    assert_eq!(row["id"], "11");
    assert_eq!(row["download_task_id"], task);
    assert!(row["offline_playlist_download"].is_object() && row["path"].is_null());
    assert!(!read_record(&profile).unwrap().to_string().contains(&task));
    assert_eq!(network.calls(), before);
    network.set(0);
    cumulus_core::download::offline::OfflineLibrary::new(session.clone())
        .unwrap()
        .refresh_membership(&session.token().unwrap())
        .await
        .unwrap();
    network.set(1);
    let licensed = client
        .playlist_page_at("73", cached_whole(), None)
        .await
        .unwrap()
        .value;
    assert_eq!(
        licensed.playlist_offline.unwrap().download_ids,
        ["11", "42"]
    );
    let before = network.calls();
    client
        .play_selection("1", play("42", false, false))
        .await
        .unwrap();
    let licensed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            if view.state == PlaybackStatus::Playing && view.play_id != playing.play_id {
                break view;
            }
            assert_ne!(view.state, PlaybackStatus::Error);
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        licensed.download_task_id.as_deref(),
        Some(encrypted.as_str())
    );
    assert_eq!(
        device.state.lock().unwrap().last_pinned.as_ref(),
        Some(&(true, std::fs::read(encrypted_path).unwrap()))
    );
    assert_eq!(network.calls(), before);
}

#[tokio::test]
async fn cached_catalog_downloads_reject_foreign_ambiguous_and_damaged_publications() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let network = Arc::new(Network::new());
    let device = Arc::new(playback_support::Device::default());
    device
        .pinned_search_supported
        .store(true, Ordering::Release);
    let native = device.clone();
    let (client, session, _) = open(&profile, network.clone(), Some(9));
    let client =
        client.with_audio_factory(move || Ok(playback_support::audio_engine(native.clone())));
    let (_, other, _) = open(&profile, network.clone(), Some(8));
    publish_fixture_song(
        &profile,
        &other,
        &temp.path().join("foreign42.flac"),
        "42",
        false,
    );
    let plain = temp.path().join("main11.flac");
    publish_fixture_song(&profile, &session, &plain, "11", false);
    let duplicate = publish_fixture_song(
        &profile,
        &session,
        &temp.path().join("extra11.flac"),
        "11",
        false,
    );
    client
        .playlist_page_at("73", cached_whole(), None)
        .await
        .unwrap();
    network.set(1);
    let page = client
        .playlist_page_at("73", cached_whole(), None)
        .await
        .unwrap()
        .value;
    assert!(page.playlist_offline.unwrap().download_ids.is_empty());
    assert!(page.tracks.iter().all(|track| track.missing));
    let revision = page.playlist_local.unwrap().revision;
    let selection = |id: &str| PlaySelection::OfflinePlaylist {
        id: "73".into(),
        revision: revision.clone(),
        start_id: Some(id.into()),
        start_local: false,
        double_click: false,
    };
    let before = network.calls();
    assert!(client.play_selection("1", selection("11")).await.is_err());
    assert!(client.play_selection("1", selection("42")).await.is_err());
    let store = ProfileRegistry::open(&profile, ApiOrigin::Pc)
        .unwrap()
        .store();
    store.delete("9", "download_tasks_v1", &duplicate).unwrap();
    let mut damaged = PUBLISHED_FLAC.to_vec();
    *damaged.last_mut().unwrap() ^= 1;
    std::fs::write(&plain, damaged).unwrap();
    let _ = client.play_selection("1", selection("11")).await;
    let failure = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            assert_ne!(view.state, PlaybackStatus::Playing);
            if view.state == PlaybackStatus::Error {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(failure.error.is_some());
    assert_eq!(
        device.state.lock().unwrap().loads,
        0,
        "same-size damaged bytes must not reach a decoder"
    );
    assert_eq!(
        network.calls(),
        before,
        "rejected publications must not authorize online"
    );
}
