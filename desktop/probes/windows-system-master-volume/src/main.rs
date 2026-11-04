#[cfg(any(all(windows, target_arch = "x86_64"), test))]
#[path = "../../../src/audio_diagnostics/windows.rs"]
mod production;

#[cfg(any(all(windows, target_arch = "x86_64"), test))]
mod report {
    use super::production::ReadStage;

    // HRESULT_FROM_WIN32(ERROR_NOT_FOUND); this only means absence at
    // GetDefaultAudioEndpoint, not at CoCreateInstance or IMMDevice::Activate.
    const E_NOTFOUND: u32 = 0x80070490;

    pub(super) fn sample(outcome: Result<(), (ReadStage, Option<u32>)>) -> String {
        use ReadStage::*;
        let Err((stage, hresult)) = outcome else {
            return r#"{"kind":"endpoint_readable","stage":null,"hresult":null}"#.into();
        };
        let kind = match stage {
            FindDefaultEndpoint if hresult == Some(E_NOTFOUND) => "default_endpoint_absent",
            InitializeCom | CreateEnumerator | FindDefaultEndpoint => "backend_unknown",
            ActivateVolume | GetScalar | InvalidScalar | GetMute => "endpoint_unreadable",
        };
        let stage = match stage {
            InitializeCom => "initialize_com",
            CreateEnumerator => "create_enumerator",
            FindDefaultEndpoint => "find_default_endpoint",
            ActivateVolume => "activate_volume",
            GetScalar => "get_scalar",
            InvalidScalar => "invalid_scalar",
            GetMute => "get_mute",
        };
        let hresult = hresult
            .map(|code| format!(r#""0x{code:08x}""#))
            .unwrap_or_else(|| "null".into());
        format!(r#"{{"kind":"{kind}","stage":"{stage}","hresult":{hresult}}}"#)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn absence_is_limited_to_the_exact_default_device_lookup_error() {
            use ReadStage::*;
            for (stage, code, kind) in [
                (FindDefaultEndpoint, E_NOTFOUND, "default_endpoint_absent"),
                (FindDefaultEndpoint, 0x88890010, "backend_unknown"),
                (InitializeCom, E_NOTFOUND, "backend_unknown"),
                (CreateEnumerator, E_NOTFOUND, "backend_unknown"),
                (ActivateVolume, E_NOTFOUND, "endpoint_unreadable"),
                (GetScalar, E_NOTFOUND, "endpoint_unreadable"),
                (GetMute, E_NOTFOUND, "endpoint_unreadable"),
            ] {
                let got = sample(Err((stage, Some(code))));
                assert!(got.starts_with(&format!(r#"{{"kind":"{kind}","#)));
                assert!(got.ends_with(&format!(r#""hresult":"0x{code:08x}"}}"#)));
            }
        }

        #[test]
        fn no_raw_values_are_needed_to_report_success_or_invalid_scalar() {
            assert_eq!(
                sample(Ok(())),
                r#"{"kind":"endpoint_readable","stage":null,"hresult":null}"#
            );
            assert_eq!(
                sample(Err((ReadStage::InvalidScalar, None))),
                r#"{"kind":"endpoint_unreadable","stage":"invalid_scalar","hresult":null}"#
            );
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    // The sample is run on this fresh process's main thread, and the returned
    // scalar and mute flag are immediately discarded. Only errors are mapped.
    println!("{}", report::sample(production::read().map(drop)));
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("A native Windows x64 process is required.");
    std::process::exit(1);
}
