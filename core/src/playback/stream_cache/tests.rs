use super::{Admission, BLOCK, Cache, Fill, Key, Lease};
use crate::{Error, playback::engine::Cancellation, storage::StateStore};
use md5::{Digest, Md5};
use std::sync::Arc;

fn open(bytes: u64) -> (tempfile::TempDir, Arc<Cache>) {
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    let cache = Cache::open(&store, bytes).unwrap();
    (root, cache)
}

fn key(owner: &str, song: &str, level: &str, data: &[u8]) -> Key {
    Key::mp3(
        owner,
        song,
        level,
        data.len() as u64,
        &hex::encode(Md5::digest(data)),
    )
    .unwrap()
}

fn fresh(cache: &Arc<Cache>, key: &Key) -> Option<Admission> {
    cache
        .admit(
            cache.revision().unwrap(),
            key.clone(),
            Cancellation::default(),
        )
        .unwrap()
}

fn fill(cache: &Arc<Cache>, key: &Key) -> Fill {
    let Some(Admission::Fill(fill)) = fresh(cache, key) else {
        panic!("expected fresh, isolated media fill")
    };
    fill.decoder_acceptance().confirm();
    fill
}

fn hit(cache: &Arc<Cache>, key: &Key) -> Lease {
    let Some(Admission::Hit(lease)) = fresh(cache, key) else {
        panic!("expected exact complete cache identity")
    };
    lease
}

#[test]
fn seek_order_partial_bytes_and_representation_fences() {
    let data = (0..BLOCK + 19).map(|i| (i % 227) as u8).collect::<Vec<_>>();
    let (_root, cache) = open(data.len() as u64 * 3);
    let original = key("11", "22", "standard", &data);
    let mut fill = fill(&cache, &original);
    fill.write_block(BLOCK as u64, &data[BLOCK..]).unwrap();
    assert!(!fill.publish_if_complete().unwrap());
    assert!(
        fresh(&cache, &original).is_none(),
        "partial data is not a hit"
    );
    let mut tail = [0; 6];
    fill.read_at(BLOCK as u64 + 4, &mut tail).unwrap();
    assert_eq!(&tail, &data[BLOCK + 4..BLOCK + 10]);
    assert!(fill.read_at(0, &mut tail).is_err());
    assert!(fill.write_block(0, &data[..BLOCK - 1]).is_err());
    fill.write_block(0, &data[..BLOCK]).unwrap();
    assert!(fill.publish_if_complete().unwrap());
    let lease = hit(&cache, &original);
    let mut crossing = [0; 8];
    lease.read_at(BLOCK as u64 - 4, &mut crossing).unwrap();
    assert_eq!(&crossing, &data[BLOCK - 4..BLOCK + 4]);
    assert!(lease.read_at(data.len() as u64, &mut crossing).is_err());
    drop(fill);
    for different in [
        key("12", "22", "standard", &data),
        key("11", "23", "standard", &data),
        key("11", "22", "higher", &data),
        key("11", "22", "standard", &vec![1; data.len()]),
    ] {
        assert!(matches!(
            fresh(&cache, &different),
            Some(Admission::Fill(_))
        ));
    }
}

#[test]
fn clear_retires_pending_and_visible_entries_without_breaking_held_playback() {
    let data = [14; 48];
    let (_root, cache) = open(96);
    let a = key("11", "22", "standard", &data);
    let b = key("11", "23", "standard", &data);
    let mut first = fill(&cache, &a);
    first.write_block(0, &data).unwrap();
    assert!(first.publish_if_complete().unwrap());
    drop(first);
    let active = hit(&cache, &a);
    let mut stale = fill(&cache, &b);
    let old = cache.revision().unwrap();
    cache.clear().unwrap();
    assert!(
        cache
            .admit(old, a.clone(), Cancellation::default())
            .unwrap()
            .is_none()
    );
    stale.write_block(0, &data).unwrap();
    assert!(
        !stale.publish_if_complete().unwrap(),
        "old fill cannot republish"
    );
    assert!(
        fresh(&cache, &a).is_none(),
        "retired leases still consume quota"
    );
    let mut retained = [0; 7];
    active.read_at(5, &mut retained).unwrap();
    assert_eq!(retained, [14; 7]);
    drop(stale);
    let next = fresh(&cache, &b);
    assert!(matches!(next, Some(Admission::Fill(_))));
    drop(next);
    drop(active);
    assert!(matches!(fresh(&cache, &a), Some(Admission::Fill(_))));
}

#[test]
fn complete_bytes_need_a_native_decoder_acceptance_before_reuse() {
    let data = [27; 32];
    let (_root, cache) = open(64);
    let key = key("11", "22", "standard", &data);
    let Some(Admission::Fill(mut refused)) = fresh(&cache, &key) else {
        panic!("new fill")
    };
    refused.write_block(0, &data).unwrap();
    assert!(refused.publish_if_complete().unwrap());
    assert!(
        fresh(&cache, &key).is_none(),
        "unaccepted decoder candidate is not a hit"
    );
    drop(refused);
    let Some(Admission::Fill(mut accepted)) = fresh(&cache, &key) else {
        panic!("refill")
    };
    accepted.write_block(0, &data).unwrap();
    assert!(accepted.publish_if_complete().unwrap());
    accepted.decoder_acceptance().confirm();
    assert!(matches!(fresh(&cache, &key), Some(Admission::Hit(_))));
}

#[test]
fn quota_evicts_only_unleased_oldest_complete_object() {
    let data = [27; 32];
    let (_root, cache) = open(64);
    let a = key("11", "22", "standard", &data);
    let b = key("11", "23", "standard", &data);
    let c = key("11", "24", "standard", &data);
    for identity in [&a, &b] {
        let mut fill = fill(&cache, identity);
        fill.write_block(0, &data).unwrap();
        assert!(fill.publish_if_complete().unwrap());
    }
    let a_reader = hit(&cache, &a);
    let b_reader = hit(&cache, &b);
    assert!(fresh(&cache, &c).is_none());
    drop(b_reader);
    let _c_fill = fill(&cache, &c);
    assert!(matches!(fresh(&cache, &a), Some(Admission::Hit(_))));
    assert!(fresh(&cache, &b).is_none());
    let mut retained = [0; 8];
    a_reader.read_at(0, &mut retained).unwrap();
    assert_eq!(retained, [27; 8]);
}

#[test]
fn shrinking_over_active_capacity_retires_pending_and_returns_held_space_on_close() {
    let (_root, cache) = open(96);
    let data = [27; 32];
    let a = key("11", "22", "standard", &data);
    let b = key("11", "23", "standard", &data);
    let mut first = fill(&cache, &a);
    first.write_block(0, &data).unwrap();
    assert!(first.publish_if_complete().unwrap());
    drop(first);
    let playing = hit(&cache, &a);
    let mut pending = fill(&cache, &b);
    cache.change_capacity(40).unwrap();
    assert_eq!(cache.usage().unwrap().reserved, 64);
    assert!(fresh(&cache, &a).is_none());
    pending.write_block(0, &data).unwrap();
    assert!(!pending.publish_if_complete().unwrap());
    let mut scratch = [0; 5];
    playing.read_at(0, &mut scratch).unwrap();
    assert_eq!(scratch, [27; 5]);
    drop(pending);
    assert_eq!(cache.usage().unwrap().reserved, 32);
    assert!(fresh(&cache, &a).is_none(), "cannot exceed reduced quota");
    drop(playing);
    assert!(matches!(fresh(&cache, &a), Some(Admission::Fill(_))));
}

#[test]
fn repeatedly_cleared_handles_remain_bounded_even_for_tiny_objects() {
    let (_root, cache) = open(1024 * 1024);
    let identity = key("11", "22", "standard", &[0; 16]);
    let mut retired = Vec::new();
    while let Some(Admission::Fill(fill)) = fresh(&cache, &identity) {
        cache.clear().unwrap();
        retired.push(fill);
        assert!(
            retired.len() <= 128,
            "retired handles cannot exceed the object bound"
        );
    }
    assert_eq!(retired.len(), 128);
    retired.pop();
    assert!(matches!(fresh(&cache, &identity), Some(Admission::Fill(_))));
}

#[test]
fn corrupt_or_revoked_fill_cannot_be_reopened_as_media() {
    let data = [11; 32];
    let (_root, cache) = open(128);
    let identity = key("11", "22", "standard", &data);
    let mut corrupt = fill(&cache, &identity);
    corrupt.write_block(0, &[12; 32]).unwrap();
    assert!(matches!(
        corrupt.publish_if_complete(),
        Err(Error::Response(_))
    ));
    assert!(matches!(
        corrupt.read_at(0, &mut [0; 4]),
        Err(Error::Cancelled)
    ));
    drop(corrupt);
    let fence = Cancellation::default();
    let Some(Admission::Fill(mut fill)) = cache
        .admit(cache.revision().unwrap(), identity.clone(), fence.clone())
        .unwrap()
    else {
        panic!("corrupt bytes cannot publish")
    };
    fill.write_block(0, &data).unwrap();
    fence.cancel();
    assert!(matches!(fill.publish_if_complete(), Err(Error::Cancelled)));
    assert!(matches!(
        fill.read_at(0, &mut [0; 4]),
        Err(Error::Cancelled)
    ));
    drop(fill);
    assert!(matches!(fresh(&cache, &identity), Some(Admission::Fill(_))));
}

#[test]
fn clear_never_walks_or_deletes_foreign_paths() {
    let data = [24; 32];
    let (root, cache) = open(64);
    let foreign = root.path().join("streaming-audio-v1").join("user-sentinel");
    std::fs::write(&foreign, b"other data").unwrap();
    let mut fill = fill(&cache, &key("11", "22", "standard", &data));
    fill.write_block(0, &data).unwrap();
    assert!(fill.publish_if_complete().unwrap());
    drop(fill);
    cache.clear().unwrap();
    drop(cache);
    assert_eq!(std::fs::read(foreign).unwrap(), b"other data");
}

#[cfg(unix)]
#[test]
fn cache_slot_refuses_an_existing_symlink() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path()).unwrap();
    std::os::unix::fs::symlink(other.path(), root.path().join("streaming-audio-v1")).unwrap();
    assert!(Cache::open(&store, 64).is_err());
    assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
}

use super::{
    StreamingAudioReader, StreamingCache,
    range::{Chunk, RangeMedia},
};
use crate::{
    account::AccountSession,
    download::transfer::MediaUrl,
    playback::{
        controller::{PlaybackCommand, PlaybackController, PlaybackStatus},
        engine::{AudioEngine, EngineSnapshot, LoadRequest, LoadedMedia, MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, SeekFrom},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Ranges {
    data: Arc<[u8]>,
    requests: Mutex<Vec<(u64, usize, bool)>>,
}
impl RangeMedia for Ranges {
    fn get<'a>(
        &'a self,
        _: &'a MediaUrl,
        offset: u64,
        length: usize,
        size: u64,
        validator: Option<&'a str>,
    ) -> ApiFuture<'a, Chunk> {
        Box::pin(async move {
            assert_eq!(size, self.data.len() as u64);
            assert_eq!(validator, (offset != 0).then_some("\"fixture-media\""));
            self.requests
                .lock()
                .unwrap()
                .push((offset, length, validator.is_some()));
            Ok(Chunk {
                bytes: self.data[offset as usize..offset as usize + length].to_vec(),
                etag: "\"fixture-media\"".into(),
            })
        })
    }
}

fn fixture_url() -> MediaUrl {
    MediaUrl::parse("https://cumulus-proof.music.126.net/stream.mp3?token=synthetic").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reader_fetches_on_seek_and_caches_only_verified_complete_bytes() {
    let mut bytes = (0..2 * BLOCK + 32)
        .map(|i| (i % 227) as u8)
        .collect::<Vec<_>>();
    bytes[..3].copy_from_slice(b"ID3");
    let data: Arc<[u8]> = bytes.into();
    let (_root, cache) = open(data.len() as u64);
    let backend = Arc::new(Ranges {
        data: data.clone(),
        requests: Mutex::default(),
    });
    let identity = key("11", "22", "standard", &data);
    let revision = cache.revision().unwrap();
    let authority = Cancellation::default();
    let mut reader = StreamingAudioReader::prepare(
        cache.clone(),
        backend.clone(),
        revision,
        identity.clone(),
        fixture_url(),
        authority.clone(),
    )
    .await
    .unwrap()
    .unwrap();
    reader.decoder_acceptance().unwrap().confirm();
    assert_eq!(*backend.requests.lock().unwrap(), [(0, BLOCK, false)]);
    let remote = backend.clone();
    let expected = data.clone();
    tokio::task::spawn_blocking(move || {
        let mut scratch = [0; 15];
        reader.seek(SeekFrom::End(-15)).unwrap();
        reader.read_exact(&mut scratch).unwrap();
        assert_eq!(&scratch, &expected[expected.len() - 15..]);
        reader.seek(SeekFrom::Start(BLOCK as u64 + 3)).unwrap();
        reader.read_exact(&mut scratch).unwrap();
        assert_eq!(&scratch, &expected[BLOCK + 3..BLOCK + 18]);
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_exact(&mut scratch).unwrap();
        assert_eq!(&scratch, &expected[..15]);
        assert_eq!(
            *remote.requests.lock().unwrap(),
            [
                (0, BLOCK, false),
                ((2 * BLOCK) as u64, 32, true),
                (BLOCK as u64, BLOCK, true)
            ]
        );
    })
    .await
    .unwrap();
    let mut reader = StreamingAudioReader::prepare(
        cache.clone(),
        backend.clone(),
        revision,
        identity,
        fixture_url(),
        authority.clone(),
    )
    .await
    .unwrap()
    .unwrap();
    cache.clear().unwrap();
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual).unwrap();
    assert_eq!(actual.as_slice(), data.as_ref());
    assert_eq!(
        backend.requests.lock().unwrap().len(),
        3,
        "hit never reaches the media backend"
    );
    authority.cancel();
    assert!(reader.seek(SeekFrom::Start(0)).is_err());
}

struct Authorizer {
    data: Arc<[u8]>,
    calls: AtomicUsize,
    revoked: AtomicBool,
}
impl Api for Authorizer {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path != "/api/song/enhance/player/url/v1" {
                return Err(Error::Response("closed streaming proof API"));
            }
            assert!(!cache);
            assert_eq!(payload["ids"], "[\"1\"]");
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.revoked.load(Ordering::SeqCst) {
                return Ok(json!({"code":403}));
            }
            Ok(json!({"code":200,"data":[{
                "id":"1","code":200,"url":fixture_url().as_str(),
                "md5":hex::encode(Md5::digest(&self.data)),
                "size":self.data.len(),"type":"mp3","fee":0,"flag":0,
                "level":"standard","expi":120
            }]}))
        })
    }
}

struct ClosedMpv(MpvEngine);
impl AudioEngine for ClosedMpv {
    fn streaming_audio_available(&mut self) -> bool {
        self.0.streaming_audio_available()
    }
    fn load_streaming_audio(
        &mut self,
        reader: StreamingAudioReader,
        cancel: &Cancellation,
    ) -> crate::Result<LoadedMedia> {
        self.0.load_streaming_audio(reader, cancel)
    }
    fn load(&mut self, _: LoadRequest, _: &Cancellation) -> crate::Result<LoadedMedia> {
        Err(Error::Response("proof refuses any stock URL load"))
    }
    fn seek(&mut self, position: f64) -> crate::Result<()> {
        self.0.seek(position)
    }
    fn volume(&mut self, value: f64) -> crate::Result<()> {
        self.0.volume(value)
    }
    fn speed(&mut self, value: f64) -> crate::Result<()> {
        self.0.speed(value)
    }
    fn stop(&mut self) -> crate::Result<()> {
        self.0.stop()
    }
    fn pause(&mut self, paused: bool) -> crate::Result<()> {
        self.0.pause(paused)
    }
    fn poll(&mut self) -> crate::Result<EngineSnapshot> {
        self.0.poll()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires real libmpv with callbacks and an explicit generated MP3 fixture"]
async fn actual_native_streaming_cache_reauthorizes_each_play() -> crate::Result<()> {
    let file = std::env::var_os("CUMULUS_PROOF_STREAM_MP3")
        .ok_or(Error::InvalidInput("supply a generated MP3 fixture"))?;
    let data: Arc<[u8]> = std::fs::read(file)?.into();
    assert!(data.len() > BLOCK && data.len() < 2 * BLOCK);
    let root = tempfile::tempdir()?;
    let store = Arc::new(StateStore::open(root.path())?);
    let cache = Cache::open(&store, data.len() as u64 * 2)?;
    let backend = Arc::new(Ranges {
        data: data.clone(),
        requests: Mutex::default(),
    });
    let api = Arc::new(Authorizer {
        data: data.clone(),
        calls: AtomicUsize::default(),
        revoked: AtomicBool::default(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session.adopt(api.clone(), json!({"userId":2}), &session.token()?)?;
    let token = session.token()?;
    let mut resolver = SourceResolver::new(session.clone(), None);
    resolver.streaming_cache = Some(Arc::new(StreamingCache {
        store: cache.clone(),
        backend: backend.clone(),
        settings: None,
    }));
    let resolver = Arc::new(resolver);
    let player = PlaybackController::start(
        session,
        store,
        resolver.clone(),
        Box::new(ClosedMpv(MpvEngine::open(MpvConfig {
            null_output: true,
            ..Default::default()
        })?)),
    )?;
    let song = || PlaybackCommand::Replace {
        resources: vec![json!({"id":"1","dt":3500})],
        index: 0,
        source: json!({"kind":"fixture"}),
    };
    let identity = key("2", "1", "standard", &data);
    let first = player.command(&token, song()).await?;
    assert_eq!(first.state, PlaybackStatus::Playing);
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if matches!(fresh(&cache, &identity), Some(Admission::Hit(_))) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("real decoder must fetch and verify the remaining range");
    let fetched = backend.requests.lock().unwrap().clone();
    assert_eq!(
        fetched,
        [(0, BLOCK, false), (BLOCK as u64, data.len() - BLOCK, true)]
    );
    let before = api.calls.load(Ordering::SeqCst);
    let second = player.command(&token, song()).await?;
    assert_eq!(second.state, PlaybackStatus::Playing);
    assert_ne!(second.play_id, first.play_id);
    assert!(api.calls.load(Ordering::SeqCst) > before);
    assert_eq!(*backend.requests.lock().unwrap(), fetched);
    let mut playback = player.subscribe();
    cache.clear()?;
    let sought = player.command(&token, PlaybackCommand::Seek(1.5)).await?;
    assert_eq!(sought.play_id, second.play_id);
    let active = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let latest = playback.borrow().clone();
            if latest.play_id == second.play_id && latest.position.is_some_and(|p| p > 1.6) {
                break latest;
            }
            playback.changed().await.unwrap();
        }
    })
    .await
    .expect("clear must not interrupt the held native stream");
    assert_eq!(active.state, PlaybackStatus::Playing);
    assert_eq!(*backend.requests.lock().unwrap(), fetched);
    // A memory-only stock authorization after clear is not a reason to
    // ask for a separate cache authorization. Simulate the next fresh source.
    resolver.invalidate(Some("1"))?;
    let third = player.command(&token, song()).await?;
    assert_eq!(third.state, PlaybackStatus::Playing);
    assert!(backend.requests.lock().unwrap().len() > fetched.len());
    tokio::time::timeout(Duration::from_secs(4), async {
        while !matches!(fresh(&cache, &identity), Some(Admission::Hit(_))) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("reopened stream should refill the cache");
    api.revoked.store(true, Ordering::SeqCst);
    let before = backend.requests.lock().unwrap().len();
    assert!(player.command(&token, song()).await.is_err());
    assert_eq!(backend.requests.lock().unwrap().len(), before);
    let requests = api.calls.load(Ordering::SeqCst);
    assert!(resolver.resolve(&json!({"id":"1"}), &token).await.is_err());
    assert!(
        api.calls.load(Ordering::SeqCst) > requests,
        "refusal cannot leave the earlier stock URL cached"
    );
    Ok(())
}
