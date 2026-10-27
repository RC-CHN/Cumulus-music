//! Official desktop MV contracts (M/15, M/21, M/232, M/334, M/3158).
//! Native models and authorizations stay independent of Tauri and the WebView.
mod api;
mod cache;
mod likes;
mod model;
pub mod playback;
mod recommendations;
mod service;
pub use api::{MvApi, MvPurpose};
pub use likes::{MvLike, MvLikeState};
pub use model::{MvArtist, MvBitrate, MvDetail, MvPrivilege};
pub use recommendations::MvRecommendation;
pub use service::{MvPosition, MvService};
