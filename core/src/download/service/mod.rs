//! Official stage ordering with account-scoped, recoverable download tasks.
mod authorize;
mod cloud;
mod control;
pub(super) mod files;
mod lyrics;
mod manager;
mod mv;
mod prepare;
mod publish;
mod records;
mod run;
mod scheduler;
mod voice;
pub use manager::DownloadService;
pub(super) use manager::{CONTROL as TASK_CONTROL, KIND as TASKS};
pub use records::{
    DownloadAction, DownloadDestination, DownloadOptions, DownloadOrigin, DownloadPhase,
    DownloadStatus, DownloadTask, LyricsStatus, OutputContainer,
};
pub use run::DownloadRunOptions;
pub use scheduler::{DownloadScheduler, ScheduledResult};
