//! Recovered local effect materials and processing primitives.
//!
//! Decoding a material does not grant an entitlement or activate a DSP graph.
//! The NCAE container accepts binary models as well as parameter JSON.

pub mod api;
pub mod authorization;
pub mod catalog;
pub mod compose;
pub mod dsp;
pub mod equalizer;
pub mod material;
pub mod ncae;
pub mod rights;
pub mod service;
