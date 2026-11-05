//! Unknown-length native PCM handoff with host-clock rendering credit.
//! It is not a media grant, full SDK graph, or a renderer command.
mod control;
mod reader;
pub(crate) use control::{Shared, StereoGraphControl, channel, native_channel};
pub use control::{StereoPcmControl, StereoPcmSnapshot};
pub use reader::StereoPcmReader;
#[cfg(test)]
mod tests;
