//! Native playback domain. Queue selection and timeline mapping do not claim
//! that an audio engine has successfully loaded the selected resource.
mod constraint;
pub mod controller;
pub mod mode;
pub(crate) mod profile_rank;
pub mod queue_more;
pub use constraint::PlaybackConstraint;
pub mod engine;
pub mod failure;
pub mod intelligence;
pub mod keep_awake;
pub mod output;
pub mod preferences;
pub mod preflight;
pub mod quality;
pub mod quality_preferences;
mod queue;
mod queue_move;
pub mod skip_policy;
pub mod source;
mod timeline;
pub(crate) use queue::{FM_BACKUP_POSITION, FM_BACKUP_QUEUE, FmRestoreOutcome, QueueResetPlan};
pub use queue::{PlaybackMode, PlaybackQueue, QueueEntry, QueueSnapshot, Step};
pub use queue_move::QueueMovement;
pub use timeline::{Timeline, TimelineMode, TimelineSnapshot};
