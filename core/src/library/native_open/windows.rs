//! Fixed local NTFS handle from admission to metadata and stock playback.
use super::{MAX_NATIVE_FILES, format_from_path, resource_from_metadata, safe_path, wav_metadata};
use crate::{
    Error, Result,
    library::{
        identity::{fingerprint_metadata, windows::WindowsFileIdentity},
        probe::Ffprobe,
    },
};
use serde_json::Value;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    os::windows::fs::FileExt,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

/// Core-owned Windows native-open batch. The queue stores lookup keys, not
/// playback authority; only the live process can resolve these rows.
pub struct WindowsNativeOpenBatch(Vec<(Value, Arc<WindowsOpenedAudio>)>);

/// An already validated file. No pathname or raw handle can be obtained from it.
pub struct WindowsOpenedAudio {
    file: Mutex<File>,
    bytes: u64,
    format: &'static str,
}

impl WindowsNativeOpenBatch {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn push(&mut self, path: PathBuf, probe: &Ffprobe) -> Result<()> {
        if self.0.len() >= MAX_NATIVE_FILES || !safe_path(&path) {
            return Err(Error::InvalidInput("invalid native local audio path"));
        }
        let format = format_from_path(&path)?;
        // MP3 metadata still requires an unverified pathname probe. Do not
        // open it at all in this native ingress lane.
        if !matches!(format, "wav" | "flac") {
            return Err(Error::InvalidInput(
                "unsupported rooted native audio format",
            ));
        }
        let (mut file, identity) = WindowsFileIdentity::open_local_fixed_ntfs_nofollow(&path)?
            .ok_or(Error::InvalidInput(
                "native audio requires a fixed local NTFS file",
            ))?;
        let fingerprint = fingerprint_metadata(file.metadata()?)?;
        let metadata = if format == "wav" {
            wav_metadata(&path, &mut file, &fingerprint)?
        } else {
            let metadata = probe.inspect_held_flac(
                &mut file,
                path.file_name()
                    .ok_or(Error::InvalidInput("missing native audio filename"))?,
            )?;
            if metadata.duration <= 0.0 || !metadata.duration.is_finite() {
                return Err(Error::Media("invalid native local audio metadata"));
            }
            metadata
        };
        if fingerprint_metadata(file.metadata()?)? != fingerprint
            || WindowsFileIdentity::from_nofollow_file(&file)? != Some(identity)
        {
            return Err(Error::State("native local audio changed"));
        }
        let bytes = fingerprint.bytes;
        let mut resource = resource_from_metadata(&path, fingerprint, metadata)?;
        resource["native_open_file"] = serde_json::json!({"held_id": Uuid::new_v4().to_string()});
        self.0.push((
            resource,
            Arc::new(WindowsOpenedAudio {
                file: Mutex::new(file),
                bytes,
                format,
            }),
        ));
        Ok(())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn resources(&self) -> Vec<Value> {
        self.0
            .iter()
            .map(|(resource, _)| resource.clone())
            .collect()
    }

    pub(crate) fn retain(&mut self, current: impl Fn(&Value) -> bool) {
        self.0.retain(|(resource, _)| current(resource));
    }

    pub(crate) fn resolve(&self, resource: &Value) -> Option<Arc<WindowsOpenedAudio>> {
        self.0
            .iter()
            .find(|(expected, _)| expected == resource)
            .map(|(_, file)| file.clone())
    }
}

impl WindowsOpenedAudio {
    pub(crate) fn format(&self) -> &'static str {
        self.format
    }

    pub(crate) fn reader(self: Arc<Self>) -> impl Read + Seek + Send + 'static {
        WindowsAudioReader {
            media: self,
            position: 0,
        }
    }
}

struct WindowsAudioReader {
    media: Arc<WindowsOpenedAudio>,
    position: u64,
}

impl Read for WindowsAudioReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.media.bytes.saturating_sub(self.position);
        let length = buffer
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        if length == 0 {
            return Ok(0);
        }
        // Windows seek_read moves the underlying File cursor. All decoders
        // use this lock and an independent logical position; the probed File
        // was moved into this owner before any readers could be made.
        let file = self
            .media
            .file
            .lock()
            .map_err(|_| io::Error::other("held native audio lock poisoned"))?;
        let read = file.seek_read(&mut buffer[..length], self.position)?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for WindowsAudioReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(offset) => self.media.bytes.checked_add_signed(offset),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
        };
        self.position = next.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid native audio seek")
        })?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::AccountSession,
        playback::{
            controller::{PlaybackCommand, PlaybackController, PlaybackStatus},
            engine::{Cancellation, MpvConfig, MpvEngine},
            source::SourceResolver,
        },
        protocol::api::{Api, ApiFuture},
        storage::StateStore,
    };
    use std::{fs, time::Duration};

    fn wave(frames: usize, sample: i16) -> Vec<u8> {
        crate::playback::engine::pcm_wave::tests::wave(std::iter::repeat_n(
            [sample, -sample],
            frames,
        ))
    }

    #[test]
    fn windows_rooted_admission_does_not_open_mp3() {
        let mut batch = WindowsNativeOpenBatch::new();
        assert!(matches!(
            batch.push(
                PathBuf::from(r"C:\nonexistent\unverified.mp3"),
                &Ffprobe::default()
            ),
            Err(Error::InvalidInput(
                "unsupported rooted native audio format"
            ))
        ));
        assert!(batch.is_empty());
    }

    #[test]
    #[ignore = "requires real Windows with a fixed local NTFS temp directory"]
    fn windows_rooted_wav_metadata_and_readers_keep_the_exact_open_object() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("合成 音乐.WAV");
        let original = wave(22_050, 256);
        fs::write(&source, &original).unwrap();
        let mut batch = WindowsNativeOpenBatch::new();
        batch.push(source.clone(), &Ffprobe::default()).unwrap();
        let mut resources = batch.resources();
        assert_eq!(resources[0]["metadata"]["duration"], 0.5);
        assert!(resources[0]["native_open_file"]["path"].is_null());
        let file = batch.resolve(&resources[0]).unwrap();
        fs::rename(&source, dir.path().join("retained.wav")).unwrap();
        fs::write(&source, wave(22_050, 1024)).unwrap();

        let mut left = file.clone().reader();
        let mut right = file.reader();
        assert_eq!(left.seek(SeekFrom::Start(45)).unwrap(), 45);
        assert_eq!(
            right.seek(SeekFrom::End(-4)).unwrap(),
            original.len() as u64 - 4
        );
        let mut bytes = [0; 4];
        right.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, original[original.len() - 4..]);
        left.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, original[45..49]);
        assert_eq!(right.read(&mut bytes).unwrap(), 0);
        assert!(
            left.seek(SeekFrom::End(-(original.len() as i64) - 1))
                .is_err()
        );

        resources[0]["metadata"]["duration"] = 4.into();
        assert!(batch.resolve(&resources[0]).is_none());
    }

    struct Offline;
    impl Api for Offline {
        fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async move { panic!("unexpected request from native media proof: {path}") })
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires real Windows fixed local NTFS and trusted package libmpv; null output"]
    async fn windows_rooted_actor_loads_held_wav_after_filename_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("first.wav");
        fs::write(&source, wave(88_200, 256)).unwrap();
        let mut batch = WindowsNativeOpenBatch::new();
        batch.push(source.clone(), &Ffprobe::default()).unwrap();
        let resources = batch.resources();
        fs::rename(&source, dir.path().join("retained.wav")).unwrap();
        fs::write(&source, wave(132_300, 1024)).unwrap();

        let store = Arc::new(StateStore::open(dir.path().join("state")).unwrap());
        let session = Arc::new(AccountSession::new(Arc::new(Offline), Some(store.clone())));
        let resolver = Arc::new(SourceResolver::new(session.clone(), None));
        let token = session.token().unwrap();
        // A persisted/display resource alone grants nothing on a fresh host.
        assert!(matches!(
            resolver.resolve(&resources[0], &token).await,
            Err(Error::StaleResponse)
        ));
        let controller = PlaybackController::start(
            session,
            store,
            resolver,
            Box::new(
                MpvEngine::open(MpvConfig {
                    null_output: true,
                    ..Default::default()
                })
                .unwrap(),
            ),
        )
        .unwrap();
        let idle = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await
            .unwrap();
        controller
            .command_guarded(
                &token,
                PlaybackCommand::ReplaceWindowsNative(batch),
                idle.fence(),
                Cancellation::default(),
            )
            .await
            .unwrap();
        let playing = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let view = controller.snapshot();
                if view.state == PlaybackStatus::Playing && view.position.is_some_and(|p| p > 0.1) {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("actual MPV clock");
        assert_eq!(playing.timeline.unwrap().total, Some(2.0));
        assert_eq!(playing.selected.unwrap().resource, resources[0]);
    }
}
