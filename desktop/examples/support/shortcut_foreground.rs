//! Synthetic two-process keyboard proof only; never linked into the client.
//! The foreground fixture may grant its exact independent producer permission
//! using the documented Win32 API, rather than disabling foreground policy.
use serde_json::Value;

#[cfg(any(windows, test))]
fn producer_id(raw: &str) -> Result<u32, String> {
    let pid = raw
        .parse::<u32>()
        .map_err(|_| "invalid keyboard producer PID")?;
    if pid == 0 || pid == u32::MAX || pid == std::process::id() {
        return Err("keyboard producer must be an independent process".into());
    }
    Ok(pid)
}

#[cfg(windows)]
pub fn grant() -> Result<Option<Value>, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, GetForegroundWindow, GetWindowThreadProcessId,
    };
    let pid = producer_id(
        &std::env::var("CUMULUS_KEY_PRODUCER_PID")
            .map_err(|_| "independent keyboard producer PID missing")?,
    )?;
    let mut foreground_pid = 0;
    // SAFETY: read the OS-owned HWND, without retaining/destroying it. The
    // output pointer names a live u32; no private window contents are read.
    unsafe {
        let foreground = GetForegroundWindow();
        if !foreground.is_null() {
            GetWindowThreadProcessId(foreground, &mut foreground_pid);
        }
    }
    if foreground_pid != 0 && foreground_pid != std::process::id() && foreground_pid != pid {
        return Err("foreground belongs to neither synthetic fixture process".into());
    }
    let granted = if foreground_pid == pid {
        false
    } else {
        // SAFETY: an explicit nonzero, non-broadcast PID from the independent
        // test launcher; this does not alter system policy or app business state.
        if unsafe { AllowSetForegroundWindow(pid) } == 0 {
            return Err("independent keyboard foreground grant denied".into());
        }
        true
    };
    Ok(Some(
        serde_json::json!({"producerPid":pid,"fixturePid":std::process::id(),"foregroundPid":foreground_pid,"granted":granted}),
    ))
}

#[cfg(not(windows))]
pub fn grant() -> Result<Option<Value>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreground_permission_is_never_broadcast_or_given_to_the_fixture_itself() {
        for value in [
            "0".into(),
            u32::MAX.to_string(),
            std::process::id().to_string(),
            "x".into(),
            "4294967296".into(),
        ] {
            assert!(producer_id(&value).is_err());
        }
        let other = if std::process::id() == 7 { 8 } else { 7 };
        assert_eq!(producer_id(&other.to_string()).unwrap(), other);
    }
}
