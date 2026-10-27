//! Login renewal from official vendor M/1368; signatures, durable intent and
//! identity lifecycle are separate. Missing real credentials stop before I/O.
mod journal;
mod lifecycle;
mod signature;
pub use lifecycle::{
    AccountLifecycle, DesktopDeviceToken, DeviceTokenProvider, LogoutResult, RefreshOutcome,
    RefreshStatus,
};
pub use signature::{DeviceInfo, encoded_model, encrypt_params, refresh_payload};
