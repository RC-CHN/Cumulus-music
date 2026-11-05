//! A live authenticated search row may reuse one published download. The
//! checked descriptor is moved to the decoder; no path is supplied to mpv.
use super::{
    OfflineRecord,
    service::{KIND, path_id},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::service::{DownloadOrigin, DownloadTask, TASKS},
    media::ncm::{HeaderPolicy, NcmReader},
    membership::{MembershipManager, MembershipState, OfflineSource, offline_access},
    playback::{
        PlaybackQueue, QueueEntry,
        engine::{Cancellation, pinned_audio_format, pinned_ncm_format},
    },
    storage::StateStore,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

// Default DownloadService's publication ceiling. Do not let damaged ledger
// bytes turn a source lookup into an unbounded scan of the file system.
const MAX_SEARCH_FILE: u64 = 512 * 1024 * 1024;

fn open_search_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A replacement FIFO cannot trap the verifier in open(), and the
        // opened leaf must be the object itself rather than a symlink target.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput(
            "search publication is not a regular file",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x0000_0400 != 0 {
            return Err(Error::InvalidInput("search publication is a reparse point"));
        }
    }
    Ok(file)
}

/// Native-only identity of a publication selected from the active account's
/// journal. Kept separately from the one-shot descriptor to allow an explicit
/// checked reopen after a native output reload.
pub struct SearchPublication {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
    token: SessionToken,
    record: OfflineRecord,
    task_target: std::path::PathBuf,
    source_gain: Option<Value>,
    ledger_snapshot: Value,
}

pub(crate) struct HeldSearchFile {
    publication: Arc<SearchPublication>,
    file: File,
    rights: Option<PlaybackWindow>,
}

#[derive(Clone)]
struct PlaybackWindow {
    monotonic: Instant,
    wall: SystemTime,
    duration: Duration,
}

impl PlaybackWindow {
    fn valid(&self) -> bool {
        self.monotonic.elapsed() < self.duration
            && SystemTime::now()
                .duration_since(self.wall)
                .is_ok_and(|elapsed| elapsed < self.duration)
    }
}

impl SearchPublication {
    pub(crate) fn bind(
        session: Arc<AccountSession>,
        token: SessionToken,
        task: &DownloadTask,
        song: &str,
    ) -> Result<Arc<Self>> {
        let store = session.persistent_store()?;
        let target = task.playback_target()?.canonicalize()?;
        let id = path_id(&target)?;
        let (task_snapshot, ledger_snapshot) = session.with_token(&token, |owner| {
            store.read_snapshot(|tx| {
                Ok((
                    tx.get(owner, TASKS, &task.id)?
                        .ok_or(Error::State("published search download is gone"))?,
                    tx.get(owner, KIND, &id)?
                        .ok_or(Error::State("published search ledger is gone"))?,
                ))
            })
        })?;
        let current: DownloadTask = serde_json::from_value(task_snapshot.clone())?;
        let record: OfflineRecord = serde_json::from_value(ledger_snapshot.clone())?;
        if task.id != current.id
            || current.origin != DownloadOrigin::Song
            || current.song_id != song
            || current.playback_target()? != task.playback_target()?
            || current.output_integrity != task.output_integrity
            || current.encrypted != task.encrypted
            || record.id != id
            || record.path != target
            || record.task_id != current.id
            || record.song_id != current.song_id
            || record.encrypted != current.encrypted
            || current.output_integrity.as_ref() != Some(&record.integrity)
        {
            return Err(Error::State("published search download identity changed"));
        }
        record.integrity.validate()?;
        if record.integrity.bytes > MAX_SEARCH_FILE {
            return Err(Error::Media(
                "published search download exceeds media bound",
            ));
        }
        Ok(Arc::new(Self {
            session,
            store,
            token,
            record,
            task_target: current.playback_target()?.to_path_buf(),
            source_gain: current
                .resource
                .as_ref()
                .and_then(|v| v.get("gain"))
                .cloned(),
            ledger_snapshot,
        }))
    }

    pub(crate) fn quality(&self) -> &'static str {
        self.record.quality.as_str()
    }

    pub(crate) fn gain(&self) -> Option<&Value> {
        self.source_gain.as_ref()
    }

    fn allows(&self, state: &MembershipState) -> Result<bool> {
        offline_access(
            Some(&self.record.privilege),
            self.record.quality,
            state,
            OfflineSource::Downloaded { complete: true },
            true,
        )
        .map(|access| access.can_play)
    }

    fn rights(&self) -> Result<Option<PlaybackWindow>> {
        if !self.record.encrypted {
            return Ok(None);
        }
        let monotonic = Instant::now();
        let wall = SystemTime::now();
        // Independent snapshot of the live session's committed account
        // evidence. Do not carry a cold/profile grant or renew membership.
        let snapshot = MembershipManager::new(self.session.clone())?.snapshot()?;
        self.session.check_generation(&self.token)?;
        if !self.allows(&snapshot.state)? {
            return Err(Error::State("published search download is not authorized"));
        }
        let mut state = snapshot.state;
        let mut boundaries = [
            state.vinyl_vip_expire_time,
            state.plus_vip_expire_time,
            state.music_package_expire_time,
            state.album_vip_expire_time,
        ];
        boundaries.sort_unstable_by(f64::total_cmp);
        for at in boundaries
            .into_iter()
            .filter(|at| *at > snapshot.server_now)
        {
            // M/224's flags are strict expiry > server_now. Re-evaluate the
            // existing M/23 policy rather than prematurely using any expiry.
            state.is_vinyl_vip = state.vinyl_vip_expire_time > at;
            state.is_plus_vip = state.plus_vip_expire_time > at;
            state.is_music_package = state.music_package_expire_time > at;
            state.is_album_vip = state.album_vip_expire_time > at;
            if !self.allows(&state)? {
                let duration = Duration::try_from_secs_f64((at - snapshot.server_now) / 1000.0)
                    .map_err(|_| Error::Response("invalid search playback expiry"))?;
                return Ok(Some(PlaybackWindow {
                    monotonic,
                    wall,
                    duration,
                }));
            }
        }
        // For example, a paid or free encrypted publication may remain
        // authorized after all membership periods have ended.
        Ok(None)
    }

    fn revalidate_publication(&self, entry: Option<&QueueEntry>) -> Result<()> {
        self.session.with_token(&self.token, |owner| {
            self.store.read_snapshot(|tx| {
                let task: DownloadTask = serde_json::from_value(
                    tx.get(owner, TASKS, &self.record.task_id)?
                        .ok_or(Error::State("published search download is gone"))?,
                )?;
                if task.id != self.record.task_id
                    || task.song_id != self.record.song_id
                    || task.origin != DownloadOrigin::Song
                    || task.playback_target()? != self.task_target
                    || task.encrypted != self.record.encrypted
                    || task.output_integrity.as_ref() != Some(&self.record.integrity)
                    || tx.get(owner, KIND, &self.record.id)?.as_ref() != Some(&self.ledger_snapshot)
                {
                    return Err(Error::State("published search download changed"));
                }
                if let Some(entry) = entry {
                    PlaybackQueue::verify_search_file_target(
                        tx,
                        owner,
                        entry,
                        &self.record.song_id,
                    )?;
                }
                Ok(())
            })
        })
    }

    fn playback_window(&self, entry: Option<&QueueEntry>) -> Result<Option<PlaybackWindow>> {
        self.revalidate_publication(entry)?;
        let rights = self.rights()?;
        self.session.check_generation(&self.token)?;
        Ok(rights)
    }

    pub(crate) fn revalidate_playback(&self, entry: Option<&QueueEntry>) -> Result<()> {
        self.playback_window(entry).map(|_| ())
    }

    pub(crate) fn open(self: &Arc<Self>, cancel: &Cancellation) -> Result<HeldSearchFile> {
        cancel.check()?;
        self.revalidate_publication(None)?;
        let _ = self.rights()?;
        if self.record.path.canonicalize()? != self.record.path {
            return Err(Error::State("published search path changed"));
        }
        let mut file = open_search_file(&self.record.path)?;
        let expected = &self.record.integrity;
        let before = file.metadata()?;
        if before.len() != expected.bytes {
            return Err(Error::Media("published search download length changed"));
        }
        let mut digest = Sha256::new();
        let mut count = 0u64;
        let mut block = [0u8; 65536];
        while count <= expected.bytes {
            cancel.check()?;
            let read = match file.read(&mut block) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if read == 0 {
                break;
            }
            count += read as u64;
            digest.update(&block[..read]);
        }
        let after = file.metadata()?;
        if count != expected.bytes
            || hex::encode(digest.finalize()) != expected.sha256
            || after.len() != before.len()
            || after.modified()? != before.modified()?
        {
            return Err(Error::Media("published search download hash changed"));
        }
        file.seek(SeekFrom::Start(0))?;
        if self.record.encrypted {
            let mut reader = NcmReader::new(&mut file, HeaderPolicy::Strict)?;
            pinned_ncm_format(&mut reader)?;
        } else {
            pinned_audio_format(&mut file)?;
        }
        file.seek(SeekFrom::Start(0))?;
        self.revalidate_publication(None)?;
        let rights = self.rights()?;
        cancel.check()?;
        Ok(HeldSearchFile {
            publication: self.clone(),
            file,
            rights,
        })
    }
}

impl HeldSearchFile {
    pub(crate) fn metadata_hint(&mut self) -> Result<crate::library::matching::MatchHint> {
        self.file.seek(SeekFrom::Start(0))?;
        crate::library::matching::opened_metadata_hint(
            &mut self.file,
            self.publication.record.encrypted,
            HeaderPolicy::Strict,
        )
    }

    pub(crate) fn into_native(
        self,
        entry: &QueueEntry,
        cancel: &Cancellation,
    ) -> Result<(File, bool, Cancellation)> {
        let Self {
            publication,
            mut file,
            rights,
        } = self;
        cancel.check()?;
        let latest = publication.playback_window(Some(entry))?;
        file.seek(SeekFrom::Start(0))?;
        let lifetime = cancel.clone();
        let authority = Cancellation::new(move || {
            !lifetime.is_cancelled()
                && rights.as_ref().is_none_or(PlaybackWindow::valid)
                && latest.as_ref().is_none_or(PlaybackWindow::valid)
        });
        authority.check()?;
        Ok((file, publication.record.encrypted, authority))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        download::{
            offline::{OfflineLibrary, PublishedDownload},
            service::{DownloadDestination, DownloadOptions, DownloadService, DownloadStatus},
        },
        media::integrity::inspect_file,
        membership::Quality,
        protocol::api::{Api, ApiFuture},
    };
    use serde_json::json;

    struct NoNetwork;
    impl Api for NoNetwork {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            panic!("sealed publication must not access an API")
        }
    }

    #[cfg(unix)]
    #[test]
    fn search_file_open_rejects_replacement_fifo_and_symlink() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let root = tempfile::tempdir().unwrap();
        let regular = root.path().join("original.flac");
        let replacement = root.path().join("replacement.flac");
        std::fs::write(&regular, b"fLaC regular file").unwrap();
        std::os::unix::fs::symlink(&regular, &replacement).unwrap();
        assert!(open_search_file(&replacement).is_err());
        std::fs::remove_file(&replacement).unwrap();

        let pathname = CString::new(replacement.as_os_str().as_bytes()).unwrap();
        // SAFETY: pathname is a live nul-terminated absolute temp path.
        assert_eq!(unsafe { libc::mkfifo(pathname.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            open_search_file(&replacement),
            Err(Error::InvalidInput(_))
        ));
        assert!(open_search_file(&regular).is_ok());
    }

    #[test]
    fn held_search_handoff_rechecks_queue_and_account_before_releasing_file() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(NoNetwork);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let token = session.token().unwrap();
        let path = root.path().join("synthetic.flac");
        let mut task = DownloadService::new(session.clone())
            .unwrap()
            .create(
                &json!(123),
                DownloadDestination::Path { path: path.clone() },
                DownloadOptions::default(),
            )
            .unwrap();
        std::fs::write(&path, b"fLaC sealed handoff header").unwrap();
        let integrity = inspect_file(&path).unwrap();
        let record = OfflineLibrary::new(session.clone())
            .unwrap()
            .register(
                &token,
                &PublishedDownload {
                    task_id: task.id.clone(),
                    song_id: "123".into(),
                    requested_quality: Quality::Standard,
                    target: path,
                    song: json!({"id":123}),
                    resource: json!({"id":123,"level":"standard"}),
                    encrypted: false,
                    integrity: integrity.clone(),
                },
            )
            .unwrap();
        task.status = DownloadStatus::Complete;
        task.target = Some(record.path);
        task.output_integrity = Some(integrity);
        store
            .put("1", TASKS, &task.id, &serde_json::to_value(&task).unwrap())
            .unwrap();
        let queue_entry = QueueEntry {
            entry_id: "selected".into(),
            resource: json!({"id":"123","trial_mode":42}),
            source: json!({"kind":"songs","scene":"search"}),
        };
        let put_queue = |entry: &QueueEntry| {
            store
                .put(
                    "1",
                    "playback",
                    "queue",
                    &json!({"items":[entry],"order":[entry.entry_id],"current":entry.entry_id}),
                )
                .unwrap();
        };
        put_queue(&queue_entry);
        let receipt =
            SearchPublication::bind(session.clone(), token.clone(), &task, "123").unwrap();
        let cancel = Cancellation::default();
        let prepared = receipt.open(&cancel).unwrap();
        let next = QueueEntry {
            entry_id: "selected-by-another-host".into(),
            ..queue_entry.clone()
        };
        put_queue(&next);
        assert!(matches!(
            prepared.into_native(&queue_entry, &cancel),
            Err(Error::QueueChanged)
        ));

        put_queue(&queue_entry);
        let prepared = receipt.open(&cancel).unwrap();
        session.adopt(api, json!({"userId":2}), &token).unwrap();
        assert!(matches!(
            prepared.into_native(&queue_entry, &cancel),
            Err(Error::StaleResponse)
        ));
    }
}
