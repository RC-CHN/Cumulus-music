//! Member service contracts. Entitlements are read from the service, not inferred
//! from the requested playback/download quality.
mod api;
pub mod center;
pub mod clock;
mod manager;
mod offline;
mod quality;
mod state;
pub use api::MembershipApi;
pub use manager::{MembershipManager, MembershipSnapshot};
pub use offline::{OfflineAccess, OfflineDenial, OfflineSource, offline_access};
pub use quality::Quality;
pub use state::{MembershipState, membership_state};
