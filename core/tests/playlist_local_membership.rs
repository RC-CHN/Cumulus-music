use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::LocalQuery},
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
}

impl Server {
    fn new() -> Self {
        Self {
            detail: Mutex::new(json!({"id":73, "name":"Synthetic", "userId":9, "privacy":0,
                "specialType":0, "trackIds":[{"id":11,"v":0},{"id":77,"v":-1},{"id":42,"v":0}],
                "description":null, "tags":[]})),
            reads: Mutex::new(Vec::new()),
        }
    }
}

impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(
                path, "/api/v6/playlist/detail",
                "local membership must never write HTTP"
            );
            assert!(!cache);
            assert_eq!(payload, json!({"id":"73","n":100000,"s":0}));
            self.reads.lock().unwrap().push(payload);
            Ok(json!({"code":200,"playlist":self.detail.lock().unwrap().clone()}))
        })
    }
}

fn open(
    path: &Path,
    api: Arc<Server>,
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
    let client =
        MusicClient::with_session(session.clone(), registry).with_local_probe(Arc::new(Probe));
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
