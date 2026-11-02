//! Official PC login verification names this computer, not a fabricated model.
pub fn name() -> String {
    let name = std::env::var("COMPUTERNAME").ok();
    #[cfg(target_os = "linux")]
    let name = {
        use std::io::Read;
        let mut value = String::new();
        if let Ok(file) = std::fs::File::open("/proc/sys/kernel/hostname")
            && file.take(1025).read_to_string(&mut value).is_ok()
            && value.len() <= 1024
        {
            Some(value.trim().into())
        } else {
            name
        }
    };
    name.filter(|v| !v.is_empty() && v.chars().count() <= 256 && !v.chars().any(char::is_control))
        .unwrap_or_else(|| "PC".into())
}
