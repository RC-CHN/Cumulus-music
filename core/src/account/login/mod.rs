//! Official credential login and explicit challenge completion.
//! One attempt owns one isolated candidate; active identity changes only after
//! account/get and an atomic profile-selection commit.
mod challenge;
mod cooldown;
mod flow;
mod protocol;
mod qr;
mod request;
mod state;
pub use challenge::{LoginStatus, VerificationAction, classify};
pub use cooldown::ResendCooldown;
pub use flow::{CredentialLogin, LoginGuardian, LoginSnapshot};
pub use protocol::LoginProtocol;
pub use qr::QrAuthentication;
pub use request::{PhoneCredential, PhoneLogin};
