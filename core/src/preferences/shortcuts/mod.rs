//! Official desktop key bindings and device persistence, without an OS hook.
mod keys;
mod model;
mod registration;
mod storage;
pub use keys::{Capture, Chord, KeyInput};
pub use model::{Action, Binding, Scope, ShortcutSettings};
pub use registration::{Hotkey, Registration, RegistrationLedger, RegistrationStatus, Ticket};

fn damaged() -> crate::Error {
    crate::Error::Json(<serde_json::Error as serde::de::Error>::custom(
        "shortcut settings damaged",
    ))
}
