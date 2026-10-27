//! Private FM from official M1384/M244/M15, with account-scoped state and
//! at-most-once feedback dispatch. Native playback integration owns the clock.
mod api;
mod blacklist;
pub mod discovery;
mod feedback;
mod model;
pub mod player;
mod reasons;
mod service;
mod trash;
mod types;
pub use api::FmApi;
pub use service::{FmClock, FmService, SystemFmClock};
pub use types::*;
