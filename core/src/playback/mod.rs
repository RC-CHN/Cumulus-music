//! Native playback domain. Queue selection and timeline mapping do not claim
//! that an audio engine has successfully loaded the selected resource.
mod constraint;
pub mod controller;
pub mod mode;
pub use constraint::PlaybackConstraint;
pub mod engine;
pub mod output;
pub mod preferences;
pub mod quality;
pub mod quality_preferences;
mod queue;
pub mod source;
mod timeline;
pub(crate) use queue::{FM_BACKUP_POSITION, FM_BACKUP_QUEUE, QueueResetPlan};
pub use queue::{PlaybackMode, PlaybackQueue, QueueEntry, QueueSnapshot, Step};
pub use timeline::{Timeline, TimelineMode, TimelineSnapshot};
