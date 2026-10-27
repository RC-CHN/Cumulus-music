//! Download policy, naming and task coordination, independent of the host UI.
pub mod completed;
pub mod directory;
pub mod naming;
pub mod offline;
pub mod service;
pub mod transfer;

mod file_identity;
pub(crate) mod playback;
