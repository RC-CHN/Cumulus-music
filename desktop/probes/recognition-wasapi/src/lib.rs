//! Offline, test-only recognition capture prototype. There is no executable entry point.

#[cfg(all(test, target_os = "windows"))]
mod native;
#[cfg(test)]
mod policy;
