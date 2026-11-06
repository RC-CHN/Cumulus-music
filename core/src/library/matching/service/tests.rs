use super::*;
use crate::{
    library::{AudioMetadata, probe::AudioProbe},
    protocol::api::{Api, ApiFuture},
};
use serde_json::json;
use std::{fs, path::Path};

struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("a matching snapshot must not call the service") })
    }
}

struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("pcm_s16le".into()),
            duration: 1.0,
            title: "合成歌曲".into(),
            album: String::new(),
            artists: Vec::new(),
            bitrate: None,
            sample_rate: Some(8000),
            channels: Some(1),
            container: "wav".into(),
            cover_bytes: None,
        })
    }
}

#[test]
fn matching_view_keeps_one_snapshot_when_a_batch_commits_after_link_read() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    fs::create_dir(&media).unwrap();
    let path = media.join("fixture.wav");
    fs::write(&path, b"synthetic local file").unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let session = Arc::new(AccountSession::new(Arc::new(Offline), Some(store.clone())));
    session
        .adopt(
            Arc::new(Offline),
            json!({"userId":9}),
            &session.token().unwrap(),
        )
        .unwrap();
    let library = LocalLibrary::with_probe(store.clone(), Arc::new(Probe)).unwrap();
    library.scan(&media).unwrap();
    let item = library.list(ListOptions::default()).unwrap().pop().unwrap();
    let ids = [item.id.clone()];
    let service = LocalMatching::new(session.clone(), library.clone()).unwrap();
    let before = service.start(&LocalQuery::default()).unwrap();
    assert_eq!((before.phase, before.total), (MatchPhase::Queued, 1));

    let mut writer = rusqlite::Connection::open(store.directory().join("core.sqlite3")).unwrap();
    // Test only: WAL lets the independent writer commit while the reader's
    // snapshot is open. Production's index lease already serializes matching
    // publishes, but the snapshot must still prevent mixing committed epochs.
    let mode: String = writer
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let mut complete = before.clone();
    complete.phase = MatchPhase::Complete;
    complete.processed = 1;
    complete.matched = 1;
    next_revision(&mut complete).unwrap();
    let mapping = LocalMapping {
        song_id: "123".into(),
        revision: item.revision.clone(),
        song: Some(json!({"id":123,"name":"合成歌曲"})),
        source: Some(MappingSource::FileMetadata),
    };

    let pinned = library
        .with_online_mappings_snapshot(&session, service.token(), &ids, |tx, uid, mappings| {
            assert!(mappings.is_empty());
            // Commit the same two records that finish publishes in one
            // transaction, at the exact old gap between mapping and job reads.
            let batch = writer.transaction().unwrap();
            batch
                .execute(
                    "INSERT INTO state(owner,kind,key,value) VALUES (?1,?2,?3,?4)",
                    (
                        uid,
                        MAPS,
                        &item.id,
                        serde_json::to_string(&mapping).unwrap(),
                    ),
                )
                .unwrap();
            assert_eq!(
                batch
                    .execute(
                        "UPDATE state SET value=?1 WHERE owner=?2 AND kind=?3 AND key=?4",
                        (
                            serde_json::to_string(&complete).unwrap(),
                            uid,
                            JOBS,
                            CURRENT
                        )
                    )
                    .unwrap(),
                1
            );
            batch.commit().unwrap();
            let committed: String = writer
                .query_row(
                    "SELECT value FROM state WHERE owner=?1 AND kind=?2 AND key=?3",
                    (uid, JOBS, CURRENT),
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<MatchJob>(&committed).unwrap().phase,
                MatchPhase::Complete
            );
            Ok(read_job(tx, uid)?.unwrap())
        })
        .unwrap();
    assert_eq!((pinned.phase, pinned.matched), (MatchPhase::Queued, 0));
    let latest = service.snapshot(&ids).unwrap();
    let job = latest.job.unwrap();
    assert_eq!((job.phase, job.matched), (MatchPhase::Complete, 1));
    assert_eq!(latest.mappings[&item.id].song_id, "123");
    assert_eq!(latest.mappings[&item.id].revision, item.revision);

    fs::write(
        &path,
        b"new revision of the same file with different length",
    )
    .unwrap();
    assert!(service.snapshot(&ids).unwrap().mappings.is_empty());
    session.detach().unwrap();
    assert!(matches!(service.snapshot(&ids), Err(Error::StaleResponse)));
}
