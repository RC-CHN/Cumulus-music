use super::*;
use cumulus_core::{
    client::dto::DesktopLyricPreferencesCommand,
    preferences::desktop_lyrics::{
        Appearance, Auxiliary, Composition, DesktopLyricPreferences, Edit, Geometry,
    },
};

#[derive(Clone, Copy)]
pub(super) struct Area {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
pub(super) fn minimum(appearance: &Appearance) -> (f64, f64) {
    let vertical = appearance.composition == Composition::Vertical;
    let lines = if appearance.single_line { 1.0 } else { 2.0 };
    let auxiliary = if appearance.auxiliary == Auxiliary::None {
        1.0
    } else {
        1.7
    };
    let content = f64::from(appearance.font_size) * 1.4 * lines * auxiliary;
    if vertical {
        (
            (32.0 + content).max(240.0),
            (90.0 + f64::from(appearance.font_size) * 1.4).max(180.0),
        )
    } else {
        (240.0, 90.0 + content + (lines - 1.0) * 4.0)
    }
}
fn bounded(saved: Option<&Geometry>, area: Area, appearance: &Appearance) -> Geometry {
    let vertical = appearance.composition == Composition::Vertical;
    let (minimum_width, minimum_height) = minimum(appearance);
    let (default_width, default_height) = if vertical {
        (minimum_width.max(280.0), 480.0)
    } else {
        (900.0, minimum_height.max(180.0))
    };
    let width = saved
        .map(|g| f64::from(g.width))
        .unwrap_or(default_width)
        .max(minimum_width)
        .min(area.width.max(240.0));
    let height = saved
        .map(|g| f64::from(g.height))
        .unwrap_or(default_height)
        .max(minimum_height)
        .min(area.height.max(80.0));
    let x = saved
        .map(|g| f64::from(g.x))
        .unwrap_or(area.x + (area.width - width) / 2.0)
        .clamp(area.x, (area.x + area.width - width).max(area.x));
    let y = saved
        .map(|g| f64::from(g.y))
        .unwrap_or(area.y + area.height - height - 40.0)
        .clamp(area.y, (area.y + area.height - height).max(area.y));
    Geometry {
        x: x.round() as i32,
        y: y.round() as i32,
        width: width.round() as u32,
        height: height.round() as u32,
    }
}
pub(super) fn fit<R: Runtime>(
    app: &tauri::AppHandle<R>,
    preferences: &DesktopLyricPreferences,
) -> Result<Geometry> {
    let main = app
        .get_webview_window("main")
        .ok_or(Error::State("main window unavailable"))?;
    let monitors = main
        .available_monitors()
        .map_err(|_| Error::State("desktop monitor unavailable"))?;
    let areas: Vec<_> = monitors
        .iter()
        .map(|m| {
            let bounds = m.work_area();
            let origin = bounds.position.to_logical::<f64>(m.scale_factor());
            let size = bounds.size.to_logical::<f64>(m.scale_factor());
            Area {
                x: origin.x,
                y: origin.y,
                width: size.width,
                height: size.height,
            }
        })
        .collect();
    let area = preferences
        .geometry
        .as_ref()
        .and_then(|g| {
            areas.iter().find(|a| {
                f64::from(g.x) >= a.x
                    && f64::from(g.y) >= a.y
                    && f64::from(g.x) < a.x + a.width
                    && f64::from(g.y) < a.y + a.height
            })
        })
        .or(areas.first())
        .copied()
        .ok_or(Error::State("desktop monitor unavailable"))?;
    Ok(bounded(
        preferences.geometry.as_ref(),
        area,
        &preferences.appearance,
    ))
}
pub(super) fn save<R: Runtime>(app: &tauri::AppHandle<R>, generation: &str) -> Result<()> {
    let label = app.state::<Host>().lock()?.get(generation)?.label.clone();
    let window = app.get_webview_window(&label).ok_or(Error::StaleResponse)?;
    let scale = window
        .scale_factor()
        .map_err(|_| Error::State("desktop lyric scale unavailable"))?;
    let position = window
        .outer_position()
        .map_err(|_| Error::State("desktop lyric position unavailable"))?
        .to_logical::<f64>(scale);
    let size = window
        .inner_size()
        .map_err(|_| Error::State("desktop lyric size unavailable"))?
        .to_logical::<f64>(scale);
    let geometry = Geometry {
        x: position.x.round() as i32,
        y: position.y.round() as i32,
        width: size.width.round() as u32,
        height: size.height.round() as u32,
    };
    let client = app.state::<MusicClient>();
    let epoch = client.account()?.epoch;
    let current = client
        .desktop_lyric_preferences(&epoch)?
        .value
        .current
        .ok_or(Error::State("desktop lyric settings damaged"))?;
    if current.geometry.as_ref() == Some(&geometry) {
        return Ok(());
    }
    app.state::<Host>().lock()?.get(generation)?;
    client.desktop_lyric_preferences_command(
        &epoch,
        DesktopLyricPreferencesCommand::Change {
            expected_revision: current.revision,
            edit: Box::new(Edit::Geometry {
                value: Some(geometry),
            }),
        },
    )?;
    Ok(())
}
pub(super) fn changed<R: Runtime>(app: &tauri::AppHandle<R>) {
    let captured = app.state::<Host>().lock().ok().and_then(|mut s| {
        let w = s.window.as_mut().filter(|w| w.ready)?;
        w.geometry_revision = w.geometry_revision.checked_add(1)?;
        Some((w.generation.clone(), w.geometry_revision))
    });
    let Some((generation, revision)) = captured else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let host = app.state::<Host>();
        let current = || {
            host.lock().ok().is_some_and(|s| {
                s.get(&generation)
                    .is_ok_and(|w| w.geometry_revision == revision)
            })
        };
        if !current() {
            return;
        }
        let _operation = host.operation.lock().await;
        if current() {
            let worker = app.clone();
            let saved_generation = generation.clone();
            let result =
                tauri::async_runtime::spawn_blocking(move || save(&worker, &saved_generation))
                    .await;
            if let Ok(mut state) = host.lock()
                && state.get(&generation).is_ok()
            {
                const MESSAGE: &str = "歌词窗口位置暂未保存，下次启动可能使用原位置";
                match result {
                    Ok(Ok(())) => {
                        if state.error == Some(MESSAGE) {
                            state.error = None;
                        }
                    }
                    Ok(Err(Error::StaleResponse)) => (),
                    _ => state.error = Some(MESSAGE),
                }
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_monitor_and_tall_fonts_keep_controls_inside_available_area() {
        let area = Area {
            x: -1280.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        let old = Geometry {
            x: 9000,
            y: 9000,
            width: 4096,
            height: 4096,
        };
        let style = Appearance {
            font_size: 96,
            single_line: false,
            ..Default::default()
        };
        let g = bounded(Some(&old), area, &style);
        assert_eq!((g.x, g.y, g.width, g.height), (-1280, 0, 1280, 720));
        let g = bounded(None, area, &style);
        assert!(g.x >= -1280 && g.x + g.width as i32 <= 0);
        assert!(g.y >= 0 && g.y + g.height as i32 <= 720);
        assert!(g.height >= 500);
        let tiny = Geometry {
            x: -1200,
            y: 30,
            width: 240,
            height: 80,
        };
        let g = bounded(Some(&tiny), area, &Appearance::default());
        assert_eq!(g.width, 240);
        assert!(
            g.height >= 176,
            "narrow two-row controls must leave room for the lyric and translation"
        );
    }
}
