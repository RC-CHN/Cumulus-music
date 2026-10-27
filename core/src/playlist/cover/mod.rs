//! Official M3094/M2167/M2131 cover editing. Cropping, durable object upload
//! and playlist binding are separate steps; a completed object is never enough
//! to claim that the playlist changed.
mod image;
mod manager;
mod run;
mod types;
use crate::{Error, Result};
pub use image::{CoverCrop, CoverImage, inspect_image};
pub use manager::PlaylistCovers;
pub use types::{CoverPhase, CoverTaskView};

const TASKS: &str = "playlist-covers-v1";
