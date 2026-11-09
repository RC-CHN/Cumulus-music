//! Read-only system output diagnostics. This does not use the player volume.
use cumulus_core::client::{ClientError, MusicClient, VolumeDiagnosisDismissal};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tauri::Manager;
use tokio::sync::Semaphore;

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemMasterVolume {
    Unsupported,
    Unavailable,
    Ready { percent: u8 },
}

type EndpointReader = dyn Fn() -> Option<(f32, bool)> + Send + Sync;

/// A desktop-only reader, with at most one OS call in flight. An isolated
/// native host can inject its own fixture before opening a WebView.
pub struct SystemMasterVolumeProbe {
    reader: Arc<EndpointReader>,
    permits: Arc<Semaphore>,
}

impl SystemMasterVolumeProbe {
    pub fn new(reader: impl Fn() -> Option<(f32, bool)> + Send + Sync + 'static) -> Self {
        Self {
            reader: Arc::new(reader),
            permits: Arc::new(Semaphore::new(1)),
        }
    }

    async fn sample_effective(&self) -> Option<f32> {
        let Ok(permit) = self.permits.clone().try_acquire_owned() else {
            return None;
        };
        let reader = self.reader.clone();
        let job = tauri::async_runtime::spawn_blocking(move || {
            // Keep the permit through OS completion even if a view closes or
            // the IPC deadline expires. Another read cannot queue behind it.
            let _permit = permit;
            reader().and_then(|(scalar, muted)| effective_scalar(scalar, muted))
        });
        match tokio::time::timeout(Duration::from_secs(2), job).await {
            Ok(Ok(value)) => value,
            _ => None,
        }
    }
}

fn effective_scalar(scalar: f32, muted: bool) -> Option<f32> {
    if !(0.0..=1.0).contains(&scalar) {
        return None;
    }
    Some(if muted { 0.0 } else { scalar })
}

fn percent(scalar: f32) -> u8 {
    // M/3097 reads the bridge's effective volume, promoted from native
    // f32 to JS Number, then applies Math.round(volume * 100).
    (f64::from(scalar) * 100.0).round() as u8
}

fn native_reader() -> Option<&'static SystemMasterVolumeProbe> {
    #[cfg(windows)]
    {
        static NATIVE: std::sync::OnceLock<SystemMasterVolumeProbe> = std::sync::OnceLock::new();
        Some(NATIVE.get_or_init(|| SystemMasterVolumeProbe::new(|| windows::read().ok())))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

async fn read_effective<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Option<f32> {
    if let Some(probe) = window.try_state::<SystemMasterVolumeProbe>() {
        return probe.sample_effective().await;
    }
    native_reader()?.sample_effective().await
}

fn require_main<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(
            cumulus_core::Error::InvalidInput("system volume requires the main window").into(),
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn system_master_volume<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
) -> Result<SystemMasterVolume, ClientError> {
    require_main(&window)?;
    if !cfg!(windows) && window.try_state::<SystemMasterVolumeProbe>().is_none() {
        return Ok(SystemMasterVolume::Unsupported);
    }
    Ok(match read_effective(&window).await {
        Some(scalar) => SystemMasterVolume::Ready {
            percent: percent(scalar),
        },
        None => SystemMasterVolume::Unavailable,
    })
}

#[tauri::command]
pub async fn check_volume_diagnosis<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    before: f64,
    after: f64,
) -> Result<Option<String>, ClientError> {
    require_main(&window)?;
    Ok(client
        .check_volume_diagnosis(&epoch, before, after, read_effective(&window))
        .await?)
}

#[tauri::command]
pub fn dismiss_volume_diagnosis<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    action: VolumeDiagnosisDismissal,
) -> Result<(), ClientError> {
    require_main(&window)?;
    Ok(client.dismiss_volume_diagnosis(&epoch, &receipt, action)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_uses_the_native_f32_before_rounding_and_mute_overrides_raw_level() {
        let effective_percent = |value, muted| effective_scalar(value, muted).map(percent);
        assert_eq!(effective_percent(0.0, false), Some(0));
        assert_eq!(effective_percent(0.375, false), Some(38));
        assert_eq!(effective_percent(0.285, false), Some(28)); // f32 promotes below 0.285
        assert_eq!(effective_percent(0.79, true), Some(0));
        assert_eq!(effective_percent(1.0, false), Some(100));
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 1.1] {
            assert_eq!(effective_percent(bad, false), None);
        }
    }
}
