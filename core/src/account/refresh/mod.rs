//! Login renewal from official vendor M/1368; signatures, durable intent and
//! identity lifecycle are separate. Missing real credentials stop before I/O.
mod journal;
mod legacy;
mod lifecycle;
mod signature;
pub(crate) use legacy::LEGACY_PERIODIC_PATH;
pub use legacy::LegacyRenewal;
pub(crate) use lifecycle::PendingReceipt;
pub use lifecycle::{
    AccountLifecycle, DesktopDeviceToken, DeviceTokenProvider, LogoutResult, RefreshOutcome,
    RefreshStatus,
};
pub use signature::{DeviceInfo, encoded_model, encrypt_params, refresh_payload};

const JSK_INTERVAL_SECS: u64 = 86_400;

// M/1368:2305: a zero time is absent, and a backward clock adjustment
// does not make the credentials eligible before the full day has passed.
fn within_jsk_interval(now: u64, last_login: Option<u64>, last_success: Option<u64>) -> bool {
    last_login
        .into_iter()
        .chain(last_success)
        .max()
        .is_some_and(|last| last != 0 && now.saturating_sub(last) < JSK_INTERVAL_SECS)
}
