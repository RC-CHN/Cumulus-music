//! Local browser measurement adapters; no remote SDK or token code is loaded.
use cumulus_core::{Error, Result, watchman::SdkBuild};

// These factories return a reviewed script or an error. Every script is recovered from
// a pinned watchman.min.js by references/recovered-client/scripts/
// recover_watchman_measurements_min.cjs, which evaluates the bundle's own function
// bodies and never initializes the SDK or touches the network.

const NO_FINGERPRINT: &str = "not a reviewed browser fingerprint measurement";

pub fn environment_factory(build: SdkBuild) -> Result<&'static str> {
    match build {
        SdkBuild::Af2952a4 => Ok(include_str!("measurements/af2952a4/environment.js")),
        SdkBuild::B602a5ad7 => Ok(include_str!("measurements/602a5ad7/environment.js")),
        SdkBuild::E2891084 => Ok(include_str!("measurements/e2891084/environment.js")),
    }
}

pub fn device_script(build: SdkBuild) -> Result<&'static str> {
    match build {
        SdkBuild::Af2952a4 => Ok(include_str!("measurements/af2952a4/device.js")),
        SdkBuild::B602a5ad7 => Ok(include_str!("measurements/602a5ad7/device.js")),
        SdkBuild::E2891084 => Ok(include_str!("measurements/e2891084/device.js")),
    }
}

pub fn fingerprint_script(build: SdkBuild, key: &str) -> Result<&'static str> {
    match (build, key) {
        (SdkBuild::Af2952a4, "Jb") => Ok(include_str!("measurements/af2952a4/plugins.js")),
        (SdkBuild::Af2952a4, "Qa") => Ok(include_str!("measurements/af2952a4/canvas.js")),
        (SdkBuild::Af2952a4, "dc") => Ok(include_str!("measurements/af2952a4/webgl.js")),
        (SdkBuild::Af2952a4, "jb") => Ok(include_str!("measurements/af2952a4/fonts.js")),
        (SdkBuild::B602a5ad7, "Jb") => Ok(include_str!("measurements/602a5ad7/plugins.js")),
        (SdkBuild::B602a5ad7, "Qa") => Ok(include_str!("measurements/602a5ad7/canvas.js")),
        (SdkBuild::B602a5ad7, "dc") => Ok(include_str!("measurements/602a5ad7/webgl.js")),
        (SdkBuild::B602a5ad7, "jb") => Ok(include_str!("measurements/602a5ad7/fonts.js")),
        (SdkBuild::E2891084, "Jb") => Ok(include_str!("measurements/e2891084/plugins.js")),
        (SdkBuild::E2891084, "Qa") => Ok(include_str!("measurements/e2891084/canvas.js")),
        (SdkBuild::E2891084, "dc") => Ok(include_str!("measurements/e2891084/webgl.js")),
        (SdkBuild::E2891084, "jb") => Ok(include_str!("measurements/e2891084/fonts.js")),
        _ => Err(Error::InvalidInput(NO_FINGERPRINT)),
    }
}
