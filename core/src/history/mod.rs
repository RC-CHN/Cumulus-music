//! Recent plays: official M/259, M/1384 and M/542. Native data stays in core.
mod api;
mod merge;
mod mutation;
pub(crate) mod playback;
mod service;
mod types;

pub use api::{HistoryApi, HistoryBoundary};
pub use merge::merge_records;
pub use mutation::{HistoryAction, HistoryMutation, HistoryMutationPhase};
pub use service::{HistoryService, HistorySnapshot, RemoteHistory};
pub use types::{HistoryKind, HistoryRecord, RecordOrigin};
