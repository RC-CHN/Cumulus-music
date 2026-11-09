//! Closed Linux proof ingress regression. No real profile or network access.
#![cfg(target_os = "linux")]

#[path = "../examples/support/signed_window_read_profile.rs"]
mod profile;
