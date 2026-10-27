//! Official M3004/M597 scene FM is an ordered voice queue, not music radio.
mod api;
mod service;
mod types;
pub use api::SceneFmApi;
pub use service::SceneFmService;
pub use types::*;
