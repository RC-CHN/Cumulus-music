use crate::{Error, Result, playback::engine::Cancellation, storage::StateStore};
use md5::{Digest, Md5};
use std::{
    collections::HashMap,
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

pub(crate) const BLOCK: usize = 64 * 1024;
const MAX_OBJECT: u64 = 8 * 1024 * 1024;
pub(super) const MAX_BUDGET: u64 = 10 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 128;
const MAX_FILLS: usize = 4;

// No URL, header, Cookie or credential revision is part of a representation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    owner: String,
    song: String,
    level: String,
    size: u64,
    digest: [u8; 16],
}

impl Key {
    pub(crate) fn mp3(owner: &str, song: &str, level: &str, size: u64, md5: &str) -> Option<Self> {
        let id = |value: &str| {
            !value.is_empty()
                && value.len() <= 32
                && !value.starts_with('0')
                && value.bytes().all(|byte| byte.is_ascii_digit())
        };
        if !id(owner)
            || !id(song)
            || !matches!(level, "standard" | "higher" | "exhigh")
            || !(12..=MAX_OBJECT).contains(&size)
            || md5.len() != 32
        {
            return None;
        }
        let mut digest = [0; 16];
        hex::decode_to_slice(md5, &mut digest).ok()?;
        Some(Self {
            owner: owner.into(),
            song: song.into(),
            level: level.into(),
            size,
            digest,
        })
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    fn block_size(&self, offset: u64) -> Option<usize> {
        if offset >= self.size || !offset.is_multiple_of(BLOCK as u64) {
            return None;
        }
        Some((self.size - offset).min(BLOCK as u64) as usize)
    }
}

struct Budget {
    reserved: AtomicU64,
    occupied: AtomicU64,
    files: AtomicUsize,
}
impl Budget {
    fn reserve(self: &Arc<Self>, bytes: u64, capacity: u64) -> Option<Reservation> {
        self.files
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_ENTRIES).then_some(count + 1)
            })
            .ok()?;
        if self
            .reserved
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(bytes).filter(|sum| *sum <= capacity)
            })
            .is_err()
        {
            self.files.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Reservation {
            budget: self.clone(),
            bytes,
            written: AtomicU64::new(0),
        })
    }
}
struct Reservation {
    budget: Arc<Budget>,
    bytes: u64,
    written: AtomicU64,
}
impl Reservation {
    fn record_write(&self, bytes: usize) {
        self.written.fetch_add(bytes as u64, Ordering::AcqRel);
        self.budget
            .occupied
            .fetch_add(bytes as u64, Ordering::AcqRel);
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget
            .occupied
            .fetch_sub(*self.written.get_mut(), Ordering::AcqRel);
        self.budget.reserved.fetch_sub(self.bytes, Ordering::AcqRel);
        self.budget.files.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct DecoderAcceptance(Arc<AtomicBool>);
impl DecoderAcceptance {
    pub(crate) fn confirm(self) {
        self.0.store(true, Ordering::Release);
    }
}

struct Payload {
    file: Mutex<File>,
    native_accepted: Arc<AtomicBool>,
    reservation: Reservation,
}
impl Payload {
    fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<()> {
        let mut file = self.file.lock().map_err(|_| Error::LockPoisoned)?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(output)?;
        Ok(())
    }

    fn write_at(&self, offset: u64, mut bytes: &[u8]) -> Result<()> {
        let mut file = self.file.lock().map_err(|_| Error::LockPoisoned)?;
        file.seek(SeekFrom::Start(offset))?;
        while !bytes.is_empty() {
            match file.write(bytes) {
                Ok(0) => return Err(std::io::Error::from(std::io::ErrorKind::WriteZero).into()),
                Ok(n) => {
                    self.reservation.record_write(n);
                    bytes = &bytes[n..];
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}
struct Record {
    payload: Arc<Payload>,
    access: u64,
}
#[derive(Default)]
struct State {
    revision: u64,
    access: u64,
    capacity: u64,
    entries: HashMap<Key, Record>,
    fills: HashMap<Key, u64>,
}

impl State {
    fn tick(&mut self) -> Result<u64> {
        self.access = self
            .access
            .checked_add(1)
            .ok_or(Error::State("cache access exhausted"))?;
        Ok(self.access)
    }

    fn retire(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::State("streaming cache revision exhausted"))?;
        self.entries.clear();
        self.fills.clear();
        Ok(())
    }

    fn evict_unleased(&mut self) -> bool {
        let oldest = self
            .entries
            .iter()
            .filter(|(_, entry)| Arc::strong_count(&entry.payload) == 1)
            .min_by_key(|(_, entry)| entry.access)
            .map(|(key, _)| key.clone());
        oldest.is_some_and(|key| self.entries.remove(&key).is_some())
    }
}

pub(crate) struct Cache {
    root: PathBuf,
    budget: Arc<Budget>,
    state: Mutex<State>,
}

pub(super) struct Usage {
    pub capacity: u64,
    pub occupied: u64,
    pub reserved: u64,
    pub reclaimable: u64,
}

pub(crate) enum Admission {
    Hit(Lease),
    Fill(Fill),
}

impl Cache {
    pub(crate) fn open(store: &StateStore, bytes: u64) -> Result<Arc<Self>> {
        if !(1..=MAX_BUDGET).contains(&bytes) {
            return Err(Error::InvalidInput(
                "streaming cache budget is out of bounds",
            ));
        }
        let parent = store.directory().canonicalize()?;
        let root = parent.join("streaming-audio-v1");
        match crate::storage::private_directory_builder().create(&root) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        validate_root(&root)?;
        Ok(Arc::new(Self {
            root,
            budget: Arc::new(Budget {
                reserved: AtomicU64::new(0),
                occupied: AtomicU64::new(0),
                files: AtomicUsize::new(0),
            }),
            state: Mutex::new(State {
                capacity: bytes,
                ..State::default()
            }),
        }))
    }

    pub(crate) fn revision(&self) -> Result<u64> {
        Ok(self.state.lock().map_err(|_| Error::LockPoisoned)?.revision)
    }

    pub(crate) fn clear(&self) -> Result<()> {
        self.state.lock().map_err(|_| Error::LockPoisoned)?.retire()
    }

    pub(super) fn directory(&self) -> &Path {
        &self.root
    }

    pub(super) fn usage(&self) -> Result<Usage> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        let reclaimable = state
            .entries
            .values()
            .filter(|entry| Arc::strong_count(&entry.payload) == 1)
            .map(|entry| entry.payload.reservation.bytes)
            .sum();
        // A retired file can be released while taking this snapshot. Reading
        // reserved before occupied preserves the bound under that release.
        let reserved = self.budget.reserved.load(Ordering::Acquire);
        let occupied = self.budget.occupied.load(Ordering::Acquire);
        Ok(Usage {
            capacity: state.capacity,
            occupied,
            reserved,
            reclaimable,
        })
    }

    pub(super) fn change_capacity(&self, capacity: u64) -> Result<()> {
        if !(1..=MAX_BUDGET).contains(&capacity) {
            return Err(Error::InvalidInput(
                "streaming cache budget is out of bounds",
            ));
        }
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if capacity < state.capacity {
            while self.budget.reserved.load(Ordering::Acquire) > capacity {
                if !state.evict_unleased() {
                    // Only active/retired handles or pending fills remain.
                    // Do not stop the player; retire their ability to become
                    // a future hit so their space is returned on close.
                    state.retire()?;
                    break;
                }
            }
        }
        state.capacity = capacity;
        Ok(())
    }

    pub(crate) fn has_hit(&self, revision: u64, key: &Key) -> Result<bool> {
        let state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(state.revision == revision
            && state
                .entries
                .get(key)
                .is_some_and(|entry| entry.payload.native_accepted.load(Ordering::Acquire)))
    }

    /// Called on a blocking I/O worker with a fresh native admission. Count
    /// reservations for in-flight fills and retired-but-still-open readers.
    pub(crate) fn admit(
        self: &Arc<Self>,
        revision: u64,
        key: Key,
        authority: Cancellation,
    ) -> Result<Option<Admission>> {
        authority.check()?;
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.revision != revision {
            return Ok(None);
        }
        let access = state.tick()?;
        if let Some(record) = state.entries.get_mut(&key) {
            if !record.payload.native_accepted.load(Ordering::Acquire) {
                // Fully fetched bytes are not yet a reusable decoder source.
                return Ok(None);
            }
            record.access = access;
            return Ok(Some(Admission::Hit(Lease {
                payload: record.payload.clone(),
                authority,
                size: key.size,
            })));
        }
        if key.size > state.capacity
            || state.fills.contains_key(&key)
            || state.fills.len() >= MAX_FILLS
        {
            return Ok(None);
        }
        while state.entries.len() + state.fills.len() >= MAX_ENTRIES {
            if !state.evict_unleased() {
                return Ok(None);
            }
        }
        let reservation = loop {
            if let Some(value) = self.budget.reserve(key.size, state.capacity) {
                break value;
            }
            if !state.evict_unleased() {
                return Ok(None);
            }
        };
        validate_root(&self.root)?;
        // This handle has no reusable name. Windows uses delete-on-close;
        // Unix unlinks the temporary inode. We never enumerate/remove files.
        let file = tempfile::tempfile_in(&self.root)?;
        authority.check()?;
        state.fills.insert(key.clone(), revision);
        let size = key.size;
        Ok(Some(Admission::Fill(Fill {
            cache: self.clone(),
            revision,
            key,
            lease: Lease {
                payload: Arc::new(Payload {
                    file: Mutex::new(file),
                    native_accepted: Arc::new(AtomicBool::new(false)),
                    reservation,
                }),
                authority,
                size,
            },
            blocks: vec![false; size.div_ceil(BLOCK as u64) as usize],
            ready: 0,
            finished: false,
        })))
    }
}

fn validate_root(root: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(root)?;
    if !meta.is_dir() || !private(&meta) || root.canonicalize()? != root {
        return Err(Error::State(
            "streaming cache requires its private app directory",
        ));
    }
    Ok(())
}

fn private(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no pointers or process mutations.
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // ACLs are inherited from the already selected application profile.
        // Do not accept junctions or other reparse points in the cache slot.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
    }
}

pub(crate) struct Lease {
    payload: Arc<Payload>,
    authority: Cancellation,
    size: u64,
}
impl Lease {
    pub(crate) fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<()> {
        self.authority.check()?;
        if offset
            .checked_add(output.len() as u64)
            .is_none_or(|end| end > self.size)
        {
            return Err(Error::InvalidInput(
                "streaming cache read is outside the source",
            ));
        }
        self.payload.read_at(offset, output)?;
        self.authority.check()
    }
}

pub(crate) struct Fill {
    cache: Arc<Cache>,
    revision: u64,
    key: Key,
    lease: Lease,
    blocks: Vec<bool>,
    ready: usize,
    finished: bool,
}
impl Fill {
    pub(crate) fn decoder_acceptance(&self) -> DecoderAcceptance {
        DecoderAcceptance(self.lease.payload.native_accepted.clone())
    }

    pub(crate) fn available(&self, offset: u64) -> bool {
        self.blocks
            .get((offset / BLOCK as u64) as usize)
            .copied()
            .unwrap_or(false)
            && offset < self.key.size
    }

    pub(crate) fn read_at(&self, offset: u64, output: &mut [u8]) -> Result<()> {
        let block = offset / BLOCK as u64 * BLOCK as u64;
        if !self.available(offset)
            || offset.saturating_add(output.len() as u64) > block + BLOCK as u64
        {
            return Err(Error::State("streaming cache block is not available"));
        }
        self.lease.read_at(offset, output)
    }

    /// No partial or duplicate blocks become reusable. A clear only retires
    /// publication; the authorized play can continue to use this open handle.
    pub(crate) fn write_block(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        self.lease.authority.check()?;
        if self.finished
            || self.key.block_size(offset) != Some(bytes.len())
            || self.available(offset)
        {
            return Err(Error::InvalidInput("invalid streaming cache block"));
        }
        self.lease
            .payload
            .write_at(offset, bytes)
            .inspect_err(|_| self.lease.authority.cancel())?;
        self.lease.authority.check()?;
        self.blocks[(offset / BLOCK as u64) as usize] = true;
        self.ready += 1;
        Ok(())
    }

    /// Checks the original source's complete media MD5 before publishing.
    /// This is a consistency check against a fresh native receipt, not an
    /// authorization mechanism or a substitute for HTTPS.
    pub(crate) fn publish_if_complete(&mut self) -> Result<bool> {
        self.lease.authority.check()?;
        if self.finished || self.ready != self.blocks.len() {
            return Ok(false);
        }
        self.finished = true;
        self.verify_and_publish()
            .inspect_err(|_| self.lease.authority.cancel())
    }

    fn verify_and_publish(&self) -> Result<bool> {
        {
            let mut file = self
                .lease
                .payload
                .file
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if file.metadata()?.len() != self.key.size {
                return Err(Error::Response("streaming cache length changed"));
            }
            file.seek(SeekFrom::Start(0))?;
            let mut hasher = Md5::new();
            let mut buffer = vec![0; BLOCK];
            let mut remaining = self.key.size;
            while remaining != 0 {
                self.lease.authority.check()?;
                let count = remaining.min(BLOCK as u64) as usize;
                file.read_exact(&mut buffer[..count])?;
                hasher.update(&buffer[..count]);
                remaining -= count as u64;
            }
            let digest: [u8; 16] = hasher.finalize().into();
            if digest != self.key.digest {
                return Err(Error::Response("streaming cache source digest mismatch"));
            }
        }
        let mut state = self.cache.state.lock().map_err(|_| Error::LockPoisoned)?;
        self.lease.authority.check()?;
        if state.revision != self.revision || state.fills.get(&self.key) != Some(&self.revision) {
            return Ok(false);
        }
        let access = state.tick()?;
        state.fills.remove(&self.key);
        state.entries.insert(
            self.key.clone(),
            Record {
                payload: self.lease.payload.clone(),
                access,
            },
        );
        Ok(true)
    }
}
impl Drop for Fill {
    fn drop(&mut self) {
        let Ok(mut state) = self.cache.state.lock() else {
            return;
        };
        if state.fills.get(&self.key) == Some(&self.revision) {
            state.fills.remove(&self.key);
        }
        // An open which never became valid cannot become a later warm hit.
        // A successor from another generation may already own this key.
        if !self.lease.payload.native_accepted.load(Ordering::Acquire)
            && state
                .entries
                .get(&self.key)
                .is_some_and(|record| Arc::ptr_eq(&record.payload, &self.lease.payload))
        {
            state.entries.remove(&self.key);
        }
    }
}
