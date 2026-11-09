//! Synthetic native focus transitions for the relay prompt proof.
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::time::{sleep, timeout};

async fn foreground(window: &WebviewWindow) -> Result<(), &'static str> {
    timeout(Duration::from_secs(2), async {
        loop {
            if cumulus_desktop::queue_tip_window_foreground(window)
                .map_err(|_| "native foreground observation failed")?
            {
                return Ok(());
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| "requested relay window never owned the native foreground")?
}

pub fn request(app: AppHandle, directory: PathBuf, outside: bool, id: u64) {
    // Creating a WebView2 helper from the synchronous event listener can
    // block the UI thread. Only this fixture owns the helper window.
    tauri::async_runtime::spawn(async move {
        let main = app.get_webview_window("main").expect("relay main window");
        let outcome = async {
            if outside {
                let other = tauri::WebviewWindowBuilder::new(
                    &app,
                    "focus-proof",
                    tauri::WebviewUrl::External("about:blank".parse().expect("blank URL")),
                )
                .title("Synthetic focus check")
                .data_directory(directory)
                .inner_size(180.0, 80.0)
                .position(8.0, 8.0)
                .build()
                .map_err(|_| "relay focus helper creation failed")?;
                other.set_focus().map_err(|_| "relay helper focus failed")?;
                foreground(&other).await?;
            } else {
                let other = app
                    .get_webview_window("focus-proof")
                    .ok_or("relay focus helper is missing")?;
                main.set_focus().map_err(|_| "relay main focus failed")?;
                #[cfg(windows)]
                {
                    // Top-level Tao activation alone does not focus the
                    // existing WebView2 controller. Wait for the actual
                    // HWND before MoveFocus, and keep the helper alive until
                    // then so closing it cannot activate a third window.
                    foreground(&main).await?;
                    main.as_ref()
                        .set_focus()
                        .map_err(|_| "relay WebView2 controller focus failed")?;
                }
                other
                    .close()
                    .map_err(|_| "relay focus helper close failed")?;
                foreground(&main).await?;
            }
            Ok(())
        }
        .await;
        let receipt = json!({
            "id": id,
            "outside": outside,
            "mainForeground": cumulus_desktop::queue_tip_window_foreground(&main).ok(),
            "helperForeground": app.get_webview_window("focus-proof")
                .and_then(|other| cumulus_desktop::queue_tip_window_foreground(&other).ok()),
            "error": outcome.err(),
        });
        main.eval(format!("window.__relayWindowFocusAck[{id}]={receipt};"))
            .expect("relay focus receipt");
    });
}
