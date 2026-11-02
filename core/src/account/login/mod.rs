//! Official credential login and explicit challenge completion.
//! One attempt owns one isolated candidate; active identity changes only after
//! account/get and an atomic profile-selection commit.
mod challenge;
mod cooldown;
mod device;
mod email;
mod flow;
pub mod presentation;
mod protocol;
mod qr;
mod question;
mod request;
mod reuse;
mod sms;
mod state;
pub use challenge::{LoginStatus, VerificationAction, classify};
pub use cooldown::ResendCooldown;
pub(crate) use device::DeviceCancellation;
pub use device::{DeviceVerificationKind, DeviceVerificationPhase, DeviceVerificationView};
pub use email::{EmailPhase, EmailVerificationView};
pub use flow::{CredentialLogin, LoginGuardian, LoginSnapshot};
pub use protocol::LoginProtocol;
pub use qr::QrAuthentication;
pub use question::{QuestionPhase, QuestionVerificationView};
pub use request::{PhoneCredential, PhoneLogin};
pub use reuse::{
    NicknameError, RegistrationBlocked, ReuseChoice, ReusePhase, ReuseVerificationView,
};
pub use sms::{SmsDirection, SmsPhase, SmsVerificationView};
