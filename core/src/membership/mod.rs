//! Member service contracts. Entitlements are read from the service, not inferred
//! from the requested playback/download quality.
mod api;
pub mod center;
pub mod clock;
mod cold;
mod lifecycle;
mod manager;
pub(crate) use cold::{ColdEntitlement, ColdMembership};
pub use lifecycle::MembershipLifecycle;
pub(crate) use manager::MembershipCommit;
mod offline;
pub mod portal;
mod quality;
mod state;
pub use api::MembershipApi;
pub use manager::{MembershipManager, MembershipSnapshot};
pub use offline::{OfflineAccess, OfflineDenial, OfflineSource, offline_access};
pub use quality::Quality;
pub use state::{MembershipState, membership_state};
