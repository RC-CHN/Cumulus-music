//! Official stage ordering with account-scoped, recoverable download tasks.
mod admission;
mod authorize;
mod batch;
mod cloud;
mod control;
mod dispatch;
pub(super) mod files;
mod lyrics;
mod manager;
mod missing_playback;
mod mv;
mod prepare;
mod publish;
mod records;
mod run;
mod scheduler;
mod voice;
pub(crate) use admission::CatalogAdmission;
pub(crate) use batch::{ExistingSongAdmission, GuardedSongBatch, SongBatchAdmission};
pub use manager::DownloadService;
pub(super) use manager::{CONTROL as TASK_CONTROL, KIND as TASKS};
pub(crate) use manager::{ScopedTaskAdmission, ScopedTaskLease};
pub(crate) use missing_playback::MissingCatalogDownload;
pub use records::{
    DownloadAction, DownloadDestination, DownloadOptions, DownloadOrigin, DownloadPhase,
    DownloadStatus, DownloadTask, LyricsStatus, OutputContainer,
};
pub use run::DownloadRunOptions;
pub use scheduler::{DownloadScheduler, ScheduledResult};
