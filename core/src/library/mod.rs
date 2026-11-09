//! Local media facts and CUE segments. Scanning and online mappings build on
//! these platform-independent contracts; probing is an explicit adapter.
pub mod catalog;
pub mod cue;
pub mod identity;
mod index;
pub mod matching;
pub mod monitor;
pub(crate) mod native_open;
pub mod probe;
mod records;
mod types;
pub use index::{IndexedFlacReceipt, LocalLibrary, LocalMapping, MappingSource};
pub(crate) use index::{delete_completed_download, relocate_download};
#[cfg(windows)]
#[doc(hidden)]
pub use native_open::{WindowsNativeOpenBatch, WindowsOpenedAudio};
pub use records::{
    LibraryItem, ListOptions, LocalResource, LocalRevision, LocalSegment, LocalSource, ScanIssue,
    ScanProgress, ScanReport, ScanStage,
};
pub use types::AudioMetadata;

pub fn supported_audio(path: &std::path::Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "mp3"
                | "flac"
                | "m4a"
                | "aac"
                | "ogg"
                | "opus"
                | "wav"
                | "aiff"
                | "ape"
                | "wma"
                | "ncm"
        )
    })
}
