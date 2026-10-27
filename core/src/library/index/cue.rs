use super::{CUES, LocalLibrary, OWNER};
use crate::{
    Error, Result,
    library::{
        AudioMetadata, LibraryItem, LocalRevision,
        cue::parse_cue,
        identity::{FileFingerprint, cue_track_id, file_id, fingerprint},
        probe::AudioProbe,
        records::CueRecord,
    },
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

struct RecordedProbe<'a> {
    inner: &'a dyn AudioProbe,
    facts: Mutex<BTreeMap<PathBuf, FileFingerprint>>,
}
impl AudioProbe for RecordedProbe<'_> {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        let before = fingerprint(path)?;
        let value = self.inner.inspect(path)?;
        if before != fingerprint(path)? {
            return Err(Error::State("CUE source changed during probe"));
        }
        self.facts
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .insert(path.into(), before);
        Ok(value)
    }
}

impl LocalLibrary {
    pub(super) fn build_cue(
        &self,
        path: &Path,
        old: Option<&CueRecord>,
    ) -> Result<(bool, CueRecord)> {
        let before = fingerprint(path)?;
        if let Some(old) = old
            && old.fingerprint == before
            && !old.missing
            && !old.unreadable
            && !old.needs_scan
            && old
                .sources
                .iter()
                .all(|(path, expected)| fingerprint(path).is_ok_and(|actual| actual == *expected))
        {
            return Ok((false, old.clone()));
        }
        let probe = RecordedProbe {
            inner: self.probe.as_ref(),
            facts: Mutex::new(BTreeMap::new()),
        };
        let sheet = parse_cue(path, &probe)?;
        let sources = probe.facts.into_inner().map_err(|_| Error::LockPoisoned)?;
        if before != fingerprint(path)?
            || sources
                .iter()
                .any(|(path, expected)| !fingerprint(path).is_ok_and(|actual| actual == *expected))
        {
            return Err(Error::State("CUE or source changed during import"));
        }
        let mut tracks = Vec::new();
        for track in sheet.tracks {
            let mut metadata = sheet.sources[&track.path].clone();
            metadata.title = if track.title.is_empty() {
                format!("Track {}", track.track)
            } else {
                track.title
            };
            metadata.album = track.album;
            metadata.artists = if track.artist.is_empty() {
                vec![]
            } else {
                vec![track.artist]
            };
            metadata.duration = track.duration;
            tracks.push(LibraryItem {
                id: cue_track_id(path, track.track)?,
                path: track.path.clone(),
                metadata,
                revision: LocalRevision::Cue {
                    sheet: path.into(),
                    sheet_fingerprint: before.clone(),
                    source_fingerprint: sources[&track.path].clone(),
                    track: track.track,
                    offset: track.offset,
                    duration: track.duration,
                },
                missing: false,
                unreadable: false,
            });
        }
        Ok((
            true,
            CueRecord {
                id: file_id(path)?,
                path: path.into(),
                fingerprint: before,
                sources,
                tracks,
                missing: false,
                unreadable: false,
                needs_scan: false,
            },
        ))
    }
    pub fn import_cue(&self, path: impl AsRef<Path>) -> Result<(bool, Vec<LibraryItem>)> {
        let _lock = self.lock()?;
        let path = path.as_ref().canonicalize()?;
        let id = file_id(&path)?;
        if path.starts_with(&self.state_root) {
            return Err(Error::InvalidInput(
                "core state directory is excluded from library",
            ));
        }
        let old: Option<CueRecord> = self
            .store
            .get(OWNER, CUES, &id)?
            .map(serde_json::from_value)
            .transpose()?;
        match self.build_cue(&path, old.as_ref()) {
            Ok((changed, record)) => {
                if changed {
                    self.store
                        .put(OWNER, CUES, &id, &serde_json::to_value(&record)?)?;
                }
                Ok((changed, record.tracks))
            }
            Err(error) => {
                if let Some(mut old) = old {
                    old.unreadable = true;
                    self.store
                        .put(OWNER, CUES, &id, &serde_json::to_value(old)?)?;
                }
                Err(error)
            }
        }
    }
}
