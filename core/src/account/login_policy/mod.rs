//! Official M/34 desktop experiment gate and M/1368 intercepted-login handling.
//! The original failed request is never replayed by this service.
mod invalid;
mod legacy;
mod policy;
pub use invalid::{
    InvalidLoginController, InvalidLoginOutcome, InvalidLoginSkip, LoginNotice, LoginNoticeKind,
    REFRESH_PATH, TokenPresence,
};
pub use legacy::{LegacyLoginController, LegacyLoginOutcome};
pub use policy::{AB_PATH, LOGIN_EXPERIMENTS, LoginExperiments, normalized_path};
