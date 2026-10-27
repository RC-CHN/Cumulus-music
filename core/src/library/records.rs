use super::{AudioMetadata, identity::FileFingerprint};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalRevision {
    File {
        fingerprint: FileFingerprint,
    },
    Cue {
        sheet: PathBuf,
        sheet_fingerprint: FileFingerprint,
        source_fingerprint: FileFingerprint,
        track: u8,
        offset: f64,
        duration: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,
    pub path: PathBuf,
    pub metadata: AudioMetadata,
    pub revision: LocalRevision,
    pub missing: bool,
    pub unreadable: bool,
}
impl LibraryItem {
    pub fn available(&self) -> bool {
        !self.missing && !self.unreadable
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct CueRecord {
    pub id: String,
    pub path: PathBuf,
    pub fingerprint: FileFingerprint,
    pub sources: BTreeMap<PathBuf, FileFingerprint>,
    pub tracks: Vec<LibraryItem>,
    pub missing: bool,
    pub unreadable: bool,
    pub needs_scan: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub error: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScanReport {
    pub root_missing: bool,
    pub updated: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub cue_updated: usize,
    pub cue_unchanged: usize,
    pub cue_missing: usize,
    pub errors: Vec<ScanIssue>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStage {
    Discovering,
    Probing,
    Committing,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanProgress {
    pub stage: ScanStage,
    pub total: usize,
    pub processed: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalResource {
    pub local_id: String,
    pub local_revision: LocalRevision,
    pub metadata: AudioMetadata,
}

#[derive(Clone, Debug, Serialize)]
pub struct LocalSource {
    pub source: PathBuf,
    pub metadata: AudioMetadata,
    pub segment: Option<LocalSegment>,
}
#[derive(Clone, Debug, Serialize)]
pub struct LocalSegment {
    pub start: f64,
    pub duration: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ListOptions<'a> {
    pub query: &'a str,
    pub include_missing: bool,
    pub include_containers: bool,
}
