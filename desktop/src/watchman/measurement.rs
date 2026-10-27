//! Local browser measurement adapters; no remote SDK or token code is loaded.
use cumulus_core::{Error, Result, watchman::SdkBuild};

pub fn environment_factory(build: SdkBuild) -> &'static str {
    match build {
        SdkBuild::Af2952a4 => include_str!("measurements/af2952a4/environment.js"),
        SdkBuild::B602a5ad7 => include_str!("measurements/602a5ad7/environment.js"),
    }
}

pub fn device_script(build: SdkBuild) -> &'static str {
    match build {
        SdkBuild::Af2952a4 => include_str!("measurements/af2952a4/device.js"),
        SdkBuild::B602a5ad7 => include_str!("measurements/602a5ad7/device.js"),
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
        _ => Err(Error::InvalidInput(
            "not a reviewed browser fingerprint measurement",
        )),
    }
}
