//! Production desktop startup + real native playback, synthetic audio only.
//! Linux CI sets LC_ALL=en_US.UTF-8 to expose GTK/libmpv locale conflicts.
#[allow(dead_code)]
#[path = "../../core/audio-filter/examples/loudness_probe.rs"]
mod loudness;
#[allow(dead_code)]
#[path = "../../core/examples/engine_roundtrip.rs"]
mod native_media;
#[allow(dead_code)]
#[path = "../../core/examples/output_device_roundtrip.rs"]
mod output_devices;

use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

struct Trace {
    file: Mutex<std::fs::File>,
    start: Instant,
}
impl Trace {
    fn record(&self, stage: &str) {
        let value = serde_json::json!({"stage":stage,"elapsedMs":self.start.elapsed().as_millis()});
        writeln!(self.file.lock().unwrap(), "{value}").unwrap();
        eprintln!("native proof: {value}");
    }
}

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
    let trace = Arc::new(Trace {
        file: Mutex::new(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output.with_extension("progress.jsonl"))
                .unwrap(),
        ),
        start: Instant::now(),
    });
    trace.record("process_entry");
    // Windows known-folder APIs do not follow an overridden LOCALAPPDATA.
    // Set the WebView directory explicitly, as the other GUI probes do.
    let webview = std::env::var_os("CUMULUS_PROOF_WEBVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| output.with_extension("webview"));
    assert!(!webview.exists(), "proof WebView directory must be new");
    let mut context = tauri::generate_context!();
    let window = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    let auto = std::env::var_os("CUMULUS_PROOF_AUTO").is_some();
    #[cfg(target_os = "linux")]
    let messages = {
        // SAFETY: single-threaded test entry, before GTK/Tauri/native workers.
        assert!(!unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) }.is_null());
        assert!(!matches!(locale(libc::LC_NUMERIC).as_str(), "C" | "POSIX"));
        locale(libc::LC_MESSAGES)
    };
    trace.record("before_gui_run");
    let setup_trace = trace.clone();
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            setup_trace.record("gui_setup");
            tauri::WebviewWindowBuilder::from_config(app, &window)?
                .data_directory(webview)
                .build()?;
            setup_trace.record("window_created");
            #[cfg(target_os = "linux")]
            {
                assert_eq!(locale(libc::LC_NUMERIC), "C");
                assert_eq!(locale(libc::LC_MESSAGES), messages);
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let result = if let Some(library) = std::env::var_os("CUMULUS_LOUDNESS_LIBRARY") {
                    setup_trace.record("loudness_start");
                    loudness::verify(&PathBuf::from(library), &output.with_extension("pcm"))
                } else if std::env::var_os("CUMULUS_OUTPUT_DEVICE_PROOF").is_some() {
                    setup_trace.record("output_devices_start");
                    output_devices::run()
                } else {
                    native_media::verify_with_observer(auto, |stage| setup_trace.record(stage))
                };
                match result {
                    Ok(mut report) => {
                        report["productionDesktopStartup"] = serde_json::json!(true);
                        report["numericLocaleCompatible"] = serde_json::json!(true);
                        std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap())
                            .unwrap();
                        setup_trace.record("report_written");
                        println!("{report}");
                        handle.exit(0);
                    }
                    Err(error) => {
                        setup_trace.record("verification_failed");
                        eprintln!("desktop native media proof failed: {error}");
                        handle.exit(1);
                    }
                }
            });
            Ok(())
        })
        .run(context)
        .expect("desktop native media proof");
    trace.record("event_loop_exited");
}
