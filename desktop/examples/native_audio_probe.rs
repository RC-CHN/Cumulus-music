//! Production desktop startup + real native playback, synthetic audio only.
//! Linux CI sets LC_ALL=en_US.UTF-8 to expose GTK/libmpv locale conflicts.
#[allow(dead_code)]
#[path = "../../core/examples/engine_roundtrip.rs"]
mod native_media;

use std::path::PathBuf;

#[cfg(target_os = "linux")]
fn locale(category: libc::c_int) -> String {
    // SAFETY: startup-only inspection; the C-owned pointer is copied immediately.
    unsafe {
        let value = libc::setlocale(category, std::ptr::null());
        assert!(!value.is_null());
        std::ffi::CStr::from_ptr(value)
            .to_string_lossy()
            .into_owned()
    }
}

fn main() {
    let output =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_REPORT").expect("explicit report path"));
    assert!(!output.exists(), "proof report must be new");
    let auto = std::env::var_os("CUMULUS_PROOF_AUTO").is_some();
    #[cfg(target_os = "linux")]
    let messages = {
        // SAFETY: single-threaded test entry, before GTK/Tauri/native workers.
        assert!(!unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) }.is_null());
        assert!(!matches!(locale(libc::LC_NUMERIC).as_str(), "C" | "POSIX"));
        locale(libc::LC_MESSAGES)
    };
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            {
                assert_eq!(locale(libc::LC_NUMERIC), "C");
                assert_eq!(locale(libc::LC_MESSAGES), messages);
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || match native_media::verify(auto) {
                Ok(mut report) => {
                    report["productionDesktopStartup"] = serde_json::json!(true);
                    report["numericLocaleCompatible"] = serde_json::json!(true);
                    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
                    println!("{report}");
                    handle.exit(0);
                }
                Err(error) => {
                    eprintln!("desktop native media proof failed: {error}");
                    handle.exit(1);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("desktop native media proof");
}
