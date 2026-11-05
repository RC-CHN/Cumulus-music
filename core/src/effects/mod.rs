//! Recovered local effect materials and processing primitives.
//!
//! Decoding a material does not grant an entitlement or activate a DSP graph.
//! The NCAE container accepts binary models as well as parameter JSON.

pub mod api;
pub mod authorization;
pub mod catalog;
pub mod compose;
pub mod device_directory;
pub mod directory;
pub mod dsp;
pub mod equalizer;
mod filter_graph;
pub mod loudness;
pub mod material;
pub mod ncae;
pub mod parametric;
mod remembered_headphone;
pub use remembered_headphone::RememberedHeadphone;
pub mod rights;
pub mod service;
pub mod stereo;
