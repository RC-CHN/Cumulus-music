//! LADSPA adapter for the recovered DSP. No network, files, decoding or Python.
//! Allocation is confined to configuration and instance creation/activation.
mod control;
mod ladspa;
mod loudness;
mod processor;
mod processor_stage;
pub use control::*;
pub use ladspa::ladspa_descriptor;
pub use loudness::*;
#[cfg(test)]
mod tests;
