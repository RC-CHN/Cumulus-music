//! Official audio/voice domains: a radio, a program and its main song are
//! distinct identities. Native records are not frontend DTOs.
mod api;
mod discovery;
mod library;
mod model;
mod mutations;
pub(crate) mod progress;
mod rights;
pub mod scene;
mod snapshot;
pub use api::{PodcastApi, ProgramPage};
pub use discovery::{DiscoveryPage, HistoryKind};
pub use library::{LibraryKind, MyLibrary};
pub use model::{Episode, Radio};
pub use mutations::{PodcastAction, PodcastMutation, PodcastService};
pub use rights::{DownloadPermission, PlayPermission};
pub use snapshot::{PodcastLibrary, PodcastScope, PodcastSnapshot};
