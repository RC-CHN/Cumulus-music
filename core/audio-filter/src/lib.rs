//! LADSPA adapter for the recovered DSP. No network, files, decoding or Python.
//! Allocation is confined to configuration and instance creation/activation.
mod control;
mod ladspa;
mod loudness;
mod parametric;
mod processor;
mod processor_stage;
mod stereo;
#[cfg(feature = "tone-geometry-proof")]
mod tone_geometry;
pub use control::*;
pub use ladspa::ladspa_descriptor;
pub use loudness::*;
pub use parametric::*;
pub use stereo::*;
#[cfg(test)]
mod tests;
