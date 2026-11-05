use super::*;
use crate::playback::{
    engine::Cancellation,
    stream_cache::{Admission, BLOCK, Key},
};
use md5::{Digest, Md5};

fn fixture() -> (tempfile::TempDir, Arc<StateStore>, Arc<Cache>, Settings) {
    let root = tempfile::tempdir().unwrap();
    let state = Arc::new(StateStore::open(root.path()).unwrap());
    let (settings, capacity) = Settings::open(state.clone()).unwrap();
    let cache = Cache::open(&state, capacity).unwrap();
    (root, state, cache, settings)
}

#[test]
fn local_capacity_survives_relaunch_and_fences_stale_controls() {
    let (root, _, cache, settings) = fixture();
    let first = settings.status(&cache).unwrap();
    assert_eq!(first.capacity_bytes, DEFAULT);
    assert_eq!(first.min_capacity_bytes, STEP);
    assert_eq!(first.max_capacity_bytes, MAX_BUDGET);
    assert_eq!(first.capacity_step_bytes, STEP);
    assert!(std::path::Path::new(&first.directory).ends_with("streaming-audio-v1"));
    assert!(matches!(
        settings.set(&cache, &first.revision, STEP + 1),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(settings.status(&cache).unwrap(), first);
    let changed = settings.set(&cache, &first.revision, 3 * STEP).unwrap();
    assert_eq!(changed.capacity_bytes, 3 * STEP);
    assert_ne!(changed.revision, first.revision);
    assert!(matches!(
        settings.clear(&cache, Some(&first.revision)),
        Err(Error::StaleResponse)
    ));
    let cleared = settings.clear(&cache, Some(&changed.revision)).unwrap();
    assert_eq!(cleared.capacity_bytes, changed.capacity_bytes);
    assert_ne!(cleared.revision, changed.revision);
    assert!(matches!(
        settings.set(&cache, &changed.revision, STEP),
        Err(Error::StaleResponse)
    ));
    let reopened = Arc::new(StateStore::open(root.path()).unwrap());
    let (settings, capacity) = Settings::open(reopened.clone()).unwrap();
    let cache = Cache::open(&reopened, capacity).unwrap();
    let restarted = settings.status(&cache).unwrap();
    assert_eq!(restarted.capacity_bytes, 3 * STEP);
    assert_eq!(restarted.directory, cleared.directory);
    assert_ne!(restarted.revision, cleared.revision);
    assert_eq!(restarted.occupied_bytes, 0);
}

#[test]
fn external_policy_update_stales_the_first_host_and_damage_is_not_overwritten() {
    let (root, original, first, settings) = fixture();
    let other = Arc::new(StateStore::open(root.path()).unwrap());
    let (external, capacity) = Settings::open(other.clone()).unwrap();
    let second = Cache::open(&other, capacity).unwrap();
    let observed = settings.status(&first).unwrap();
    let changed = external
        .set(&second, &external.status(&second).unwrap().revision, STEP)
        .unwrap();
    assert!(matches!(
        settings.set(&first, &observed.revision, 3 * STEP),
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        settings.status(&first).unwrap().capacity_bytes,
        changed.capacity_bytes
    );
    original
        .transaction(|tx| tx.delete(OWNER, KIND, CURRENT))
        .unwrap();
    assert!(matches!(
        settings.status(&first),
        Err(Error::State("streaming cache preferences are damaged"))
    ));
    assert!(Settings::open(original.clone()).is_err());
    assert!(original.get(OWNER, KIND, CURRENT).unwrap().is_none());
}

#[test]
fn usage_separates_written_reclaimable_and_held_bytes_without_spurious_revision_changes() {
    let (_root, _, cache, settings) = fixture();
    let data = vec![37; BLOCK + 17];
    let key = Key::mp3(
        "2",
        "8",
        "standard",
        data.len() as u64,
        &hex::encode(Md5::digest(&data)),
    )
    .unwrap();
    let first = settings.status(&cache).unwrap();
    let Some(Admission::Fill(mut fill)) = cache
        .admit(
            cache.revision().unwrap(),
            key.clone(),
            Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("new fill")
    };
    fill.write_block(0, &data[..BLOCK]).unwrap();
    let pending = settings.status(&cache).unwrap();
    assert_eq!(pending.revision, first.revision);
    assert_eq!(pending.reserved_bytes, data.len() as u64);
    assert_eq!(pending.occupied_bytes, BLOCK as u64);
    assert_eq!(pending.reclaimable_bytes, 0);
    fill.write_block(BLOCK as u64, &data[BLOCK..]).unwrap();
    fill.decoder_acceptance().confirm();
    assert!(fill.publish_if_complete().unwrap());
    drop(fill);
    let complete = settings.status(&cache).unwrap();
    assert_eq!(complete.revision, first.revision);
    assert_eq!(complete.reclaimable_bytes, data.len() as u64);
    let Some(Admission::Hit(lease)) = cache
        .admit(cache.revision().unwrap(), key, Cancellation::default())
        .unwrap()
    else {
        panic!("expected complete hit")
    };
    let held = settings.clear(&cache, Some(&first.revision)).unwrap();
    assert_ne!(held.revision, first.revision);
    assert_eq!(held.reclaimable_bytes, 0);
    assert_eq!(held.occupied_bytes, data.len() as u64);
    let mut byte = [0];
    lease.read_at(0, &mut byte).unwrap();
    assert_eq!(byte, [37]);
    drop(lease);
    let released = settings.status(&cache).unwrap();
    assert_eq!(released.occupied_bytes, 0);
    assert_eq!(released.reserved_bytes, 0);
    assert_eq!(released.revision, held.revision);
}
