//! LADSPA adapter for the recovered DSP. No network, files, decoding or Python.
//! Allocation is confined to configuration and instance creation/activation.
mod control;
mod ladspa;
mod processor;
pub use control::*;
pub use ladspa::ladspa_descriptor;
#[cfg(test)]
mod tests;
