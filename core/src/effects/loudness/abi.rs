//! Independent configuration handle for the recovered loudness LADSPA node.
//! Settings are scalar copies. Statistics share the established filter layout.
pub use crate::effects::equalizer::abi::{CAPACITY, EqualizerStats as LoudnessStats, MAX_ID};
pub const VERSION: u32 = 1;
pub type Version = unsafe extern "C" fn() -> u32;
pub type Create = unsafe extern "C" fn(f32, u32, *mut u32) -> i32;
pub type Update = unsafe extern "C" fn(u32, f32, u32) -> i32;
pub type Stats = unsafe extern "C" fn(u32, *mut LoudnessStats) -> i32;
pub type Release = unsafe extern "C" fn(u32) -> i32;
