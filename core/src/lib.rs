//! Platform-independent application services.
//!
//! This crate must not depend on Tauri, a webview, or a desktop event loop.
//! Hosts own platform integration; the core owns music and account state.

pub mod account;
mod application;
pub mod catalog;
pub mod client;
pub mod cloud;
pub mod comments;
pub mod download;
pub mod effects;
pub mod error;
pub mod fm;
pub mod history;
pub mod identity;
pub mod library;
pub mod listen_together;
pub mod lyrics;
pub mod media;
pub mod membership;
pub mod mv;
pub mod playback;
pub mod playlist;
pub mod podcast;
pub mod preferences;
pub mod protocol;
pub mod relay;
pub mod storage;
pub mod watchman;

pub use application::{AppInfo, Application};
pub use error::{Error, Result};
