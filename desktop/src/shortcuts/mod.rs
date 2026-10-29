//! Device shortcut intent, actual OS registrations and captured core actions.
mod dispatch;
mod host;
mod native;
pub mod settings;
pub(crate) use host::stop;
pub use host::{Phase, Row, Status, install, status};
