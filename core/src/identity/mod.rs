//! Recovered native clientSign and device records. Hardware collection is a
//! separate host adapter; none of these values authenticate an account.
mod codec;
mod compatibility;
mod composition;
mod container;
mod facts;
mod service;
pub use codec::{STORAGE_KEY, decode_record, encode_record, storage_transform};
pub use compatibility::{WINDOWS_COMPATIBILITY_DESCRIPTION, WindowsBrowserIdentity};
pub use composition::{client_sign, derive_persistent_id, valid_persistent_id};
pub use container::{
    DeviceRecordSummary, ID_RECORD_KEY, MARKER, MARKER_RECORD_KEY, MAX_CONTAINER_BYTES,
    encode_container, inspect_container, parse_container, read_container,
};
pub use facts::{HardwareFacts, IdentitySource, IdentitySummary, collect_linux_identity};
pub use service::{DeviceIdentity, IdentityService};
