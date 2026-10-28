//! Official privacy/notification settings: M/3097 + M/3136. A full readback
//! matters because playRecordSetting changes several linked privacy fields.
mod legacy;
pub(crate) use legacy::compatibility_record;
mod mutation;
mod service;
mod types;
pub use service::AccountSettings;
pub use types::*;
