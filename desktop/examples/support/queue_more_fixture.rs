//! Isolated two-read recommendation fixture. No profile data or API leaves the host.
use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::{
    io::Write,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tauri::Manager;
use tokio::sync::Notify;

const MORE: &str = "/api/homepage/more/rcmd/song";
const PRIVILEGE: &str = "/api/song/enhance/privilege";

#[derive(Default)]
struct Fixture {
    hold: AtomicBool,
    wake: Notify,
    requests: Mutex<Vec<Value>>,
    fetches: AtomicUsize,
}

fn fixture() -> &'static Fixture {
    static VALUE: OnceLock<Fixture> = OnceLock::new();
    VALUE.get_or_init(Fixture::default)
}

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_QUEUE_MORE_UI_PROOF").is_some()
}

pub fn tip_enabled() -> bool {
    std::env::var_os("CUMULUS_QUEUE_MORE_TIP_PROOF").is_some()
}

/// Bounded stderr breadcrumbs for the isolated Windows cue processes.
pub fn phase_milestone(stage: &'static str) {
    static COUNT: AtomicU8 = AtomicU8::new(0);
    if !cfg!(windows)
        || !tip_enabled()
        || COUNT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < 64).then_some(count + 1)
            })
            .is_err()
    {
        return;
    }
    let phase = if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
        "reopen"
    } else {
        "first"
    };
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "queue-tip-{phase}:{stage}");
    let _ = stderr.flush();
}

/// Only the test creates this second native window. Neither this request
/// nor its receipt can change the product's focus or cue predicates.
pub fn request_tip_focus(app: &tauri::AppHandle, focus: bool, sequence: u64) {
    let main = app.get_webview_window("main").unwrap();
    let helper = app.get_window("queue-tip-focus-helper").unwrap();
    tauri::async_runtime::spawn(async move {
        phase_milestone("focus_enter");
        let result = async {
            if focus {
                #[cfg(not(windows))]
                helper.hide().map_err(|_| "focus_window_request_failed")?;
                main.set_focus()
                    .map_err(|_| "focus_window_request_failed")?;
                #[cfg(windows)]
                {
                    focus_windows_webview(&main).await?;
                    // Keep the helper visible until the main HWND owns the
                    // foreground; hiding the foreground helper earlier can
                    // activate an unrelated top-level window.
                    helper.hide().map_err(|_| "focus_window_request_failed")?;
                }
            } else {
                helper.show().map_err(|_| "focus_window_request_failed")?;
                helper
                    .set_focus()
                    .map_err(|_| "focus_window_request_failed")?;
            }

            let observed = tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    if main.is_focused().ok() == Some(focus)
                        && main.is_visible().ok() == Some(true)
                        && main.is_minimized().ok() == Some(false)
                        && (focus || helper.is_focused().ok() == Some(true))
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            observed.map_err(|_| "focus_native_mismatch")?;
            Ok(())
        }
        .await;
        let category = result.err().unwrap_or("focus_native_observed");
        phase_milestone(category);
        let receipt = json!({
            "sequence": sequence,
            "desired": focus,
            "outcome": category,
            "nativeFocused": main.is_focused().ok(),
            "nativeVisible": main.is_visible().ok(),
            "helperFocused": helper.is_focused().ok(),
        });
        main.eval(format!("window.__queueTipFocusAck={receipt};"))
            .unwrap();
        phase_milestone("focus_returned");
    });
}

#[cfg(windows)]
async fn focus_windows_webview(
    main: &tauri::WebviewWindow,
) -> std::result::Result<(), &'static str> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let window = main.hwnd().map_err(|_| "focus_window_request_failed")?.0 as usize;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            // SAFETY: this read-only Win32 query has no inputs or ownership
            // transfer. Compare its opaque handle with this test's own HWND.
            if unsafe { GetForegroundWindow() } as usize == window {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| "focus_foreground_mismatch")?;
    phase_milestone("focus_foreground_observed");
    // WebviewWindow::set_focus only activates the top-level Tao window.
    // The separate Webview dispatcher invokes the existing WebView2
    // controller's MoveFocus on the UI thread without building a WebView.
    main.as_ref()
        .set_focus()
        .map_err(|_| "focus_controller_request_failed")?;
    phase_milestone("focus_controller_requested");
    Ok(())
}

pub fn renderer_milestone(stage: &str) {
    let stage = match stage {
        "renderer_enter" => "renderer_enter",
        "daily_ready" => "daily_ready",
        "daily_load_begin" => "daily_load_begin",
        "daily_playing" => "daily_playing",
        "recommendations_loaded" => "recommendations_loaded",
        "focus_observed" => "focus_observed",
        "focus_renderer_mismatch" => "focus_renderer_mismatch",
        "recommendation_playing" => "recommendation_playing",
        "cue_check_begin" => "cue_check_begin",
        "cue_denied" => "cue_denied",
        "second_recommendation_playing" => "second_recommendation_playing",
        "capture_acknowledged" => "capture_acknowledged",
        _ => return,
    };
    phase_milestone(stage);
}

pub fn capture_milestone(stage: &str) {
    let stage = match stage {
        "ux_01_unfocused" => "capture_written_unfocused",
        "ux_02_at_zero" => "capture_written_at_zero",
        "ux_03_past_window" => "capture_written_past_window",
        "ux_04_shown" => "capture_written_shown",
        "ux_05_seen" => "capture_written_seen",
        "ux_06_another_rec" => "capture_written_another_rec",
        "ux_01_restart_suppressed" => "capture_written_restart_suppressed",
        _ => return,
    };
    phase_milestone(stage);
}

pub fn groups() -> Result<PcExperimentGroups> {
    let group = if std::env::var_os("CUMULUS_QUEUE_MORE_CONTROL").is_some() {
        "c"
    } else {
        "t1"
    };
    PcExperimentGroups::refreshed(&json!({"data":[
        {"expName":"PH-PC-tagT","expGroupName":"c"},
        {"expName":"PH-PC-recommendMore","expGroupName":group}
    ]}))
}

pub fn control(mode: &str) -> Result<Value> {
    let fixture = fixture();
    match mode {
        "hold" => fixture.hold.store(true, Ordering::SeqCst),
        "release" => {
            fixture.hold.store(false, Ordering::SeqCst);
            fixture.wake.notify_waiters();
        }
        "read" => {}
        _ => {
            return Err(Error::InvalidInput(
                "unknown queue recommendation fixture mode",
            ));
        }
    }
    Ok(report())
}

pub fn report() -> Value {
    let fixture = fixture();
    json!({"requests": fixture.requests.lock().unwrap().clone(),
           "held":fixture.hold.load(Ordering::SeqCst)})
}

fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id}"),"ar":[{"id":7,"name":"合成歌手"}],
           "al":{"id":8,"name":"合成专辑"},"dt":if tip_enabled() {45000} else {30000},"fee":0})
}

pub async fn response(path: &str, args: &Value, cache: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let fixture = fixture();
    match path {
        "/api/v3/discovery/recommend/songs" => Some(Ok(json!({
            "code":200,"data":{"dailySongs":[song(1),song(2)]}
        }))),
        MORE => {
            assert_eq!(args["source"], "curlist_more_rcmd");
            assert_eq!(args.as_object().unwrap().len(), 3);
            assert!(args["songIds"].is_array());
            fixture.requests.lock().unwrap().push(json!({
                "path":path,"args":args,"cache":cache
            }));
            let batch = fixture.fetches.fetch_add(1, Ordering::SeqCst);
            let first = 51 + batch as u64 * 10;
            loop {
                let wake = fixture.wake.notified();
                tokio::pin!(wake);
                wake.as_mut().enable();
                if !fixture.hold.load(Ordering::SeqCst) {
                    break;
                }
                wake.await;
            }
            let songs = (first..=first + 3).map(song).collect::<Vec<_>>();
            let alg = (first..=first + 3)
                .map(|id| (id.to_string(), json!("synthetic-more")))
                .collect::<serde_json::Map<_, _>>();
            Some(Ok(
                json!({"code":200,"data":{"songList":songs,"algMap":alg}}),
            ))
        }
        PRIVILEGE => {
            assert_eq!(args.as_object().unwrap().len(), 1);
            let raw = args["ids"].as_str().expect("privilege IDs JSON string");
            let ids: Vec<Value> = serde_json::from_str(raw).expect("privilege IDs array");
            assert_eq!(
                ids.len(),
                3,
                "official upper bound applies before privilege reads"
            );
            fixture.requests.lock().unwrap().push(json!({
                "path":path,"args":args,"cache":cache
            }));
            Some(Ok(
                json!({"code":200,"data": ids.into_iter().map(|id| json!({
                "id":id, "st":0,"fee":0,"cp":1,"fl":320000,"pl":320000,"dl":320000
            })).collect::<Vec<_>>()}),
            ))
        }
        _ => None,
    }
}
