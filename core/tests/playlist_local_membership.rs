#[path = "playlist_local_membership/duration.rs"]
mod duration;
#[path = "playlist_local_membership/offline.rs"]
mod offline;
#[path = "playlist_local_membership/queue.rs"]
mod queue;
#[path = "playlist_local_membership/reorder.rs"]
mod reorder;
#[path = "playlist_local_membership/song_comment.rs"]
mod song_comment;

#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback_support;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{LocalQuery, MixedPlaylistStart, PageRequest, PlaySelection, PlayerResourceKind},
    },
    library::{AudioMetadata, LocalLibrary, probe::AudioProbe},
    playlist::{
        PlaylistCommand, PlaylistLocalChange, PlaylistLocalMembership, PlaylistLocalRef,
        PlaylistLocalRow, PlaylistLocalStore, TrackChange,
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            title: path.file_stem().unwrap().to_string_lossy().into(),
            duration: 61.0,
            album: String::new(),
            artists: vec!["Synthetic".into()],
            format: Some("wav".into()),
            container: "wav".into(),
            bitrate: None,
            sample_rate: None,
            channels: None,
            cover_bytes: None,
        })
    }
}

struct Server {
    detail: Mutex<Value>,
    reads: Mutex<Vec<Value>>,
    songs: Mutex<Vec<Vec<String>>>,
    hold_songs: std::sync::atomic::AtomicBool,
    songs_started: tokio::sync::Notify,
    release_songs: tokio::sync::Notify,
}

impl Server {
    fn new() -> Self {
        Self {
            detail: Mutex::new(json!({"id":73, "name":"Synthetic", "userId":9, "privacy":0,
                "specialType":0, "trackIds":[{"id":11,"v":0},{"id":77,"v":-1},{"id":42,"v":0}],
                "description":null, "tags":[]})),
            reads: Mutex::new(Vec::new()),
            songs: Mutex::new(Vec::new()),
            hold_songs: Default::default(),
            songs_started: Default::default(),
            release_songs: Default::default(),
        }
    }
}

impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                "/api/v6/playlist/detail" => {
                    assert!(!cache);
                    assert_eq!(payload, json!({"id":"73","n":100000,"s":0}));
                    self.reads.lock().unwrap().push(payload);
                    Ok(json!({"code":200,"playlist":self.detail.lock().unwrap().clone()}))
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["c"].as_str().unwrap()).unwrap();
                    let ids: Vec<_> = ids
                        .iter()
                        .map(|row| row["id"].as_str().unwrap().to_owned())
                        .collect();
                    assert!(
                        ids.iter().all(|id| ["11", "42"].contains(&id.as_str())),
                        "a local member must never become a catalog ID"
                    );
                    self.songs.lock().unwrap().push(ids.clone());
                    if self.hold_songs.load(std::sync::atomic::Ordering::Acquire) {
                        self.songs_started.notify_one();
                        self.release_songs.notified().await;
                    }
                    let songs: Vec<_> = ids.iter().map(|id| json!({"id":id,"name":format!("Online {id}"),"dt":61000,"ar":[],"al":{"name":"Album"}})).collect();
                    Ok(json!({"code":200,"songs":songs}))
                }
                "/api/song/enhance/player/url/v1" => {
                    // The real actor can prefetch its next catalog item. Keep
                    // the URL unusable and reject every non-catalog identity.
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    let [id] = ids.as_slice() else {
                        panic!("expected a single audio ID")
                    };
                    assert!(
                        ["11", "42"].contains(&id.as_str().unwrap_or_default())
                            || matches!(id.as_u64(), Some(11 | 42))
                    );
                    Ok(json!({"code":200,"data":[{"id":id,"code":404,"url":null}]}))
                }
                _ => panic!("unexpected remote operation: {path}"),
            }
        })
    }
}

fn open(
    path: &Path,
    api: Arc<dyn Api>,
    uid: Option<u32>,
) -> (MusicClient, Arc<AccountSession>, LocalLibrary) {
    let registry = ProfileRegistry::open(path, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    if let Some(uid) = uid {
        session
            .adopt(api, json!({"userId":uid}), &session.token().unwrap())
            .unwrap();
    }
    let library = LocalLibrary::with_probe(registry.store(), Arc::new(Probe)).unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_local_probe(Arc::new(Probe))
        .with_audio_factory(|| {
            Ok(playback_support::audio_engine(Arc::new(
                playback_support::Device::default(),
            )))
        });
    (client, session, library)
}

fn project(session: &Arc<AccountSession>, server: &Server) -> PlaylistLocalMembership {
    PlaylistLocalStore::new(session.clone())
        .unwrap()
        .project(
            &session.token().unwrap(),
            "73",
            &server.detail.lock().unwrap(),
        )
        .unwrap()
}

async fn refs(client: &MusicClient) -> Vec<PlaylistLocalRef> {
    client
        .local_page(LocalQuery {
            descending: false,
            ..Default::default()
        })
        .await
        .unwrap()
        .tracks
        .into_iter()
        .map(|row| PlaylistLocalRef {
            id: row.track.id,
            revision: row.revision,
        })
        .collect()
}

fn add(rows: &[PlaylistLocalRef]) -> PlaylistLocalChange {
    PlaylistLocalChange::Add {
        rows: rows.to_vec(),
    }
}
fn remove(rows: &[PlaylistLocalRef]) -> PlaylistLocalChange {
    PlaylistLocalChange::Remove {
        rows: rows.to_vec(),
    }
}

#[tokio::test]
async fn local_membership_survives_reopen_and_never_turns_stale_files_into_new_references() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("a.wav"), b"synthetic file a").unwrap();
    std::fs::write(media.join("b.wav"), b"synthetic file b").unwrap();
    std::fs::write(
        media.join("a.cue"),
        "FILE \"a.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"a\"\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let server = Arc::new(Server::new());

    let (first_revision, a, b) = {
        let (client, session, library) = open(&profile, server.clone(), Some(9));
        let report = library.scan(&media).unwrap();
        assert_eq!(report.cue_updated, 1, "{:?}", report.errors);
        let rows = refs(&client).await;
        let (a, b) = (rows[0].clone(), rows[1].clone());
        let initial = project(&session, &server);
        assert_eq!(initial.rows.len(), 3);
        let added = client
            .mutate_playlist_local("1", "73", &initial.revision, add(&[b.clone(), a.clone()]))
            .await
            .unwrap()
            .value;
        assert!(added.changed);
        assert_eq!(
            added.membership.rows,
            [
                PlaylistLocalRow::Local(b.clone()),
                PlaylistLocalRow::Local(a.clone()),
                PlaylistLocalRow::Remote { index: 0 },
                PlaylistLocalRow::Remote { index: 1 },
                PlaylistLocalRow::Remote { index: 2 },
            ]
        );
        assert_ne!(added.membership.revision, initial.revision);
        let duplicate = client
            .mutate_playlist_local(
                "1",
                "73",
                &added.membership.revision,
                add(&[b.clone(), a.clone()]),
            )
            .await
            .unwrap()
            .value;
        assert!(!duplicate.changed);
        assert_eq!(duplicate.membership, added.membership);
        assert!(matches!(
            client
                .mutate_playlist_local(
                    "1",
                    "73",
                    &initial.revision,
                    remove(std::slice::from_ref(&a))
                )
                .await,
            Err(Error::QueueChanged)
        ));
        (added.membership.revision, a, b)
    };

    let (client, session, library) = open(&profile, server.clone(), Some(9));
    assert_eq!(project(&session, &server).revision, first_revision);
    server.detail.lock().unwrap()["trackIds"] = json!([{"id":88,"v":0},{"id":42,"v":0}]);
    assert!(matches!(
        client
            .mutate_playlist_local("1", "73", &first_revision, remove(std::slice::from_ref(&a)))
            .await,
        Err(Error::QueueChanged)
    ));
    let current = project(&session, &server);
    assert_eq!(
        current.rows,
        [
            PlaylistLocalRow::Local(b.clone()),
            PlaylistLocalRow::Local(a.clone()),
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Remote { index: 1 },
        ]
    );

    let editor = client.playlist_editor("1", "73").await.unwrap().value;
    assert!(matches!(
        client
            .mutate_playlist(
                "1",
                PlaylistCommand::Tracks {
                    id: "73".into(),
                    revision: editor.revision,
                    change: TrackChange::Clear,
                    ids: vec![],
                }
            )
            .await,
        Err(Error::State(
            "local-only playlist entries require native matching before editing"
        ))
    ));

    std::fs::write(
        media.join("a.wav"),
        b"synthetic replacement a with a different size",
    )
    .unwrap();
    assert!(matches!(
        client
            .mutate_playlist_local("1", "73", &current.revision, add(std::slice::from_ref(&a)))
            .await,
        Err(Error::QueueChanged)
    ));
    let report = library.scan(&media).unwrap();
    assert_eq!(report.cue_updated, 1, "{report:?}");
    let current_a = refs(&client)
        .await
        .into_iter()
        .find(|row| row.id == a.id)
        .unwrap();
    assert_ne!(current_a.revision, a.revision, "{report:?}");
    assert!(matches!(
        client
            .mutate_playlist_local(
                "1",
                "73",
                &current.revision,
                remove(std::slice::from_ref(&current_a))
            )
            .await,
        Err(Error::QueueChanged)
    ));
    let removed = client
        .mutate_playlist_local("1", "73", &current.revision, remove(&[a, b]))
        .await
        .unwrap()
        .value;
    assert!(removed.changed);
    assert_eq!(
        removed.membership.rows,
        [
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Remote { index: 1 }
        ]
    );
    assert_eq!(project(&session, &server), removed.membership);
    let mut original_server = server.detail.lock().unwrap();
    original_server["trackIds"] = json!([{"id":11,"v":0},{"id":77,"v":-1},{"id":42,"v":0}]);
    drop(original_server);
    assert_ne!(project(&session, &server).revision, first_revision);
}

#[tokio::test]
async fn local_membership_is_scoped_and_never_adopts_an_unbound_server_local_row() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("a.wav"), b"synthetic local file").unwrap();
    let server = Arc::new(Server::new());
    let (client, session, library) = open(&profile, server.clone(), Some(9));
    library.scan(&media).unwrap();
    let rows = refs(&client).await;
    let initial = project(&session, &server);
    assert_eq!(initial.rows[1], PlaylistLocalRow::Remote { index: 1 });
    client
        .mutate_playlist_local("1", "73", &initial.revision, add(&rows))
        .await
        .unwrap();
    let reads = server.reads.lock().unwrap().len();
    let malformed = PlaylistLocalRef {
        id: "77".into(),
        revision: rows[0].revision.clone(),
    };
    assert!(matches!(
        client
            .mutate_playlist_local("1", "73", &initial.revision, add(&[malformed]))
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(server.reads.lock().unwrap().len(), reads);

    let (other, other_session, _) = open(&profile, server.clone(), Some(10));
    assert_eq!(project(&other_session, &server), initial);
    assert!(matches!(
        other
            .mutate_playlist_local("1", "73", &initial.revision, add(&rows))
            .await,
        Err(Error::InvalidInput(
            "this operation requires a regular owned playlist"
        ))
    ));
    server.detail.lock().unwrap()["specialType"] = json!(5);
    assert_eq!(project(&session, &server), initial);
    assert!(matches!(
        client
            .mutate_playlist_local("1", "73", &initial.revision, add(&rows))
            .await,
        Err(Error::InvalidInput(
            "this operation requires a regular owned playlist"
        ))
    ));
    let (guest, _, _) = open(&profile, server.clone(), None);
    let before_guest = server.reads.lock().unwrap().len();
    assert!(
        guest
            .mutate_playlist_local("0", "73", &initial.revision, add(&rows))
            .await
            .is_err()
    );
    assert_eq!(server.reads.lock().unwrap().len(), before_guest);
}

#[tokio::test]
async fn owned_mixed_pages_and_native_queue_keep_local_ids_out_of_remote_reads() {
    use cumulus_core::membership::Quality;

    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let file = media.join("local-track.wav");
    std::fs::write(&file, b"synthetic fixture audio").unwrap();
    let server = Arc::new(Server::new());
    let (client, _, library) = open(&temp.path().join("profile"), server.clone(), Some(9));
    library.scan(&media).unwrap();
    let reference = refs(&client).await.remove(0);

    let req = |offset, revision| PageRequest {
        offset,
        limit: 2,
        revision,
    };
    let initial = client
        .playlist_page_at("73", req(0, None), None)
        .await
        .unwrap()
        .value;
    let baseline = initial.playlist_local.unwrap();
    assert!(!baseline.has_members && baseline.rows.is_empty());
    let added = client
        .mutate_playlist_local(
            "1",
            "73",
            &baseline.revision,
            add(std::slice::from_ref(&reference)),
        )
        .await
        .unwrap()
        .value;
    let revision = added.membership.revision.clone();
    let first = client
        .playlist_page_at("73", req(0, None), None)
        .await
        .unwrap()
        .value;
    assert_eq!(first.revision, initial.revision);
    assert_eq!(first.total, 4);
    assert_eq!(
        first
            .tracks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        [reference.id.as_str(), "11"]
    );
    assert!(first.tracks[0].local && !first.tracks[0].missing);
    assert_eq!(first.tracks[0].name, "local-track");
    let local = first.playlist_local.unwrap();
    assert_eq!(local.revision, revision);
    assert!(local.has_members);
    assert_eq!(local.rows.get(&reference.id), Some(&reference.revision));
    assert!(matches!(
        client
            .playlist_page_at("73", req(2, first.revision.clone()), None)
            .await,
        Err(Error::QueueChanged)
    ));
    let second = client
        .playlist_page_at("73", req(2, first.revision.clone()), Some(revision.clone()))
        .await
        .unwrap()
        .value;
    assert_eq!(
        second
            .tracks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["77", "42"]
    );
    assert!(second.tracks[0].local && second.tracks[0].missing);
    assert!(second.playlist_local.unwrap().rows.is_empty());

    let selected = |double_click| PlaySelection::MixedPlaylist {
        id: "73".into(),
        revision: revision.clone(),
        start: Some(MixedPlaylistStart {
            id: reference.id.clone(),
            local: true,
        }),
        double_click,
        quality: Quality::Standard,
    };
    client.play_selection("1", selected(false)).await.unwrap();
    let playing = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let view = client.player_snapshot("1").await.unwrap();
            if view.state == cumulus_core::playback::controller::PlaybackStatus::Playing {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(playing.resource_kind, Some(PlayerResourceKind::Local));
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|entry| entry.track.id.as_str())
            .collect::<Vec<_>>(),
        [reference.id.as_str(), "11", "42"]
    );
    let paused = client.play_selection("1", selected(true)).await.unwrap();
    assert_eq!(paused.play_id, playing.play_id);
    assert_eq!(paused.queue_revision, playing.queue_revision);
    assert_eq!(
        paused.state,
        cumulus_core::playback::controller::PlaybackStatus::Paused
    );
    assert!(
        client
            .mutate_playlist_local(
                "1",
                "73",
                &revision,
                remove(std::slice::from_ref(&reference))
            )
            .await
            .unwrap()
            .value
            .changed
    );
    let after = client.player_snapshot("1").await.unwrap();
    assert_eq!(after.play_id, playing.play_id);
    assert_eq!(after.queue_revision, playing.queue_revision);
    assert!(matches!(
        client
            .playlist_page_at("73", req(2, first.revision), Some(revision))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(std::fs::read(file).unwrap(), b"synthetic fixture audio");
    assert_eq!(
        *server.songs.lock().unwrap(),
        [
            vec!["11".to_owned()],
            vec!["11".to_owned()],
            vec!["42".to_owned()],
            vec!["11".to_owned(), "42".to_owned()],
            vec!["11".to_owned(), "42".to_owned()],
        ],
    );
}
