use super::*;
use cumulus_core::preferences::desktop_lyrics::DesktopLyricPreferences;
use tauri::webview::PageLoadEvent;

pub(super) async fn open<R: Runtime>(
    app: &tauri::AppHandle<R>,
    preferences: DesktopLyricPreferences,
) -> Result<()> {
    let (generation, label) = app.state::<Host>().lock()?.begin()?;
    let worker = app.clone();
    let build_generation = generation.clone();
    let building = tauri::async_runtime::spawn_blocking(move || -> Result<()> {
        let position = geometry::fit(&worker, &preferences)?;
        let (minimum_width, minimum_height) = geometry::minimum(&preferences.appearance);
        let reload_generation = build_generation.clone();
        let url = format!("index.html#desktop-lyrics={build_generation}");
        let window =
            tauri::WebviewWindowBuilder::new(&worker, label, tauri::WebviewUrl::App(url.into()))
                .title("Cumulus · 桌面歌词")
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .visible(false)
                .focused(false)
                .skip_taskbar(true)
                .maximizable(false)
                .inner_size(f64::from(position.width), f64::from(position.height))
                .min_inner_size(
                    minimum_width.min(f64::from(position.width)),
                    minimum_height.min(f64::from(position.height)),
                )
                .position(f64::from(position.x), f64::from(position.y))
                .always_on_top(preferences.appearance.always_on_top)
                .on_page_load(move |webview, event| {
                    if event.event() != PageLoadEvent::Started {
                        return;
                    }
                    let reload = webview
                        .state::<Host>()
                        .lock()
                        .ok()
                        .and_then(|mut s| {
                            s.get_mut(&reload_generation).ok().map(|w| {
                                let loaded = w.loaded;
                                w.loaded = true;
                                loaded
                            })
                        })
                        .unwrap_or(false);
                    if reload {
                        dismiss(webview.app_handle(), Some(&reload_generation));
                        if let Ok(mut s) = webview.state::<Host>().lock() {
                            s.error = Some("歌词窗口已重载，请在设置中重新打开");
                        }
                    }
                })
                .build()
                .map_err(|_| Error::State("desktop lyric window creation failed"))?;
        if worker
            .state::<Host>()
            .lock()?
            .get(&build_generation)
            .is_err()
        {
            let _ = window.destroy();
            return Err(Error::StaleResponse);
        }
        Ok(())
    });
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        building
            .await
            .map_err(|_| Error::State("desktop lyric creation stopped"))??;
        loop {
            if app.state::<Host>().lock()?.get(&generation)?.ready {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }
    })
    .await
    .unwrap_or(Err(Error::State("desktop lyric startup timed out")));
    if result.is_err() {
        gui(app, move |app| {
            dismiss(&app, Some(&generation));
            Ok(())
        })
        .await?;
        app.state::<Host>().lock()?.error = Some("桌面歌词暂时无法打开，请在设置中重试");
    }
    result
}
