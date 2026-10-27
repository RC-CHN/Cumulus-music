//! Login and identity lifecycle. A QR authorization still requires account/get.
mod api;
pub mod credential_import;
pub mod credentials;
pub mod login;
pub mod login_policy;
pub mod profiles;
pub mod qr;
pub mod refresh;
pub mod resources;
pub mod session;
pub use api::{AccountApi, LikeContext, validate_settings};
pub use qr::{QrLogin, QrSnapshot, QrStatus};
pub use session::{AccountSession, SessionSnapshot, SessionStatus, SessionToken};
