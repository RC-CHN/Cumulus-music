//! Synthetic-proof-only handshake: do not advance the script until PNG saving ends.
use std::{path::Path, time::Duration};
use tauri::Manager;
pub fn script(source: &str) -> String {
    format!(
        "{}\n{}",
        include_str!("capture_ack.js"),
        source.replace(
            "window.__TAURI_INTERNALS__.invoke",
            "window.__CUMULUS_PROOF_INVOKE__"
        )
    )
}
pub async fn acknowledge(
    app: &tauri::AppHandle,
    output: &Path,
    stage: &str,
    opted_in: bool,
) -> bool {
    if std::env::var("CUMULUS_PROOF_CAPTURE_ACK").as_deref() != Ok("1") || !stage.starts_with("ux_")
    {
        return true;
    }
    let marker = output.join(format!("{stage}.captured"));
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.is_file() {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await;
    if opted_in
        && result.is_ok()
        && let Some(window) = app.get_webview_window("main")
        && window
            .eval(format!(
                "window.__CUMULUS_PROOF_ACK__={};",
                serde_json::json!(stage)
            ))
            .is_ok()
    {
        return true;
    }
    let failure =
        serde_json::json!({"ok":false,"stage":stage,"error":"screenshot acknowledgement failed"});
    let _ = std::fs::write(
        output.join("failed.json"),
        serde_json::to_vec_pretty(&failure).unwrap(),
    );
    app.exit(1);
    false
}
