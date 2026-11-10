//! Official audio/voice domains: a radio, a program and its main song are
//! distinct identities. Native records are not frontend DTOs.
mod api;
mod discovery;
mod library;
mod model;
mod mutations;
pub(crate) mod progress;
mod purchased;
pub(crate) mod radio_order;
pub(crate) mod resume;
pub use purchased::{PurchasedItem, PurchasedKind};
mod rights;
pub mod scene;
mod skip;
mod snapshot;
pub use api::{PodcastApi, ProgramPage};
pub use discovery::{DiscoveryPage, HistoryKind};
pub(crate) use library::LikedVoicePage;
pub use library::{LibraryKind, MyLibrary};
pub use model::{Episode, Radio};
pub(crate) use mutations::FreshVinylSubscription;
pub use mutations::{PodcastAction, PodcastMutation, PodcastService};
pub use rights::{DownloadPermission, PlayPermission};
pub use skip::{AudiobookIntervals, AudiobookSkip, is_audiobook};
pub(crate) use snapshot::cache_generation_in;
pub use snapshot::{PodcastLibrary, PodcastScope, PodcastSnapshot};
