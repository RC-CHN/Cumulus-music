use cumulus_core::{
    client::{ClientError, MusicClient},
    effects::{
        catalog::{CatalogueKind, EffectId},
        device_directory::EffectDeviceRow,
        directory::{EffectDirectoryDetail, EffectDirectoryOverview, EffectDirectoryRow},
        service::DeviceMaterialCheck,
    },
    playback::engine::{MpvConfig, MpvEngine, TrustedFlacRuntime},
};
use std::path::{Path, PathBuf};

pub(super) fn library(resources: &Path) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CUMULUS_AUDIO_FILTER") {
        return Some(path.into());
    }
    let filename = if cfg!(target_os = "windows") {
        "cumulus_audio_filter.dll"
    } else if cfg!(target_os = "macos") {
        "libcumulus_audio_filter.dylib"
    } else {
        "libcumulus_audio_filter.so"
    };
    let executable = std::env::current_exe().ok();
    resolve_library(resources, executable.as_deref(), filename)
}

fn resolve_library(resources: &Path, executable: Option<&Path>, filename: &str) -> Option<PathBuf> {
    // The portable package ships a sibling. On Linux Tauri may fall back
    // to /usr/lib/Cumulus for a portable executable, which can belong to a
    // different Debian installation. Prefer this executable's own filter.
    if let Some(sibling) = executable
        .and_then(Path::parent)
        .map(|directory| directory.join(filename))
        .filter(|candidate| candidate.is_file())
    {
        return Some(sibling);
    }
    let bundled = resources.join("native").join(filename);
    bundled.is_file().then_some(bundled)
}

fn packaged_flac(resources: &Path) -> Option<TrustedFlacRuntime> {
    #[cfg(target_os = "linux")]
    const PACKAGED: Option<(&str, &str)> = match option_env!("CUMULUS_PACKAGED_LINUX_FLAC_SHA256") {
        Some(sha) => Some(("libcumulus_flac150.so.14", sha)),
        None => None,
    };
    #[cfg(target_os = "windows")]
    const PACKAGED: Option<(&str, &str)> = match option_env!("CUMULUS_PACKAGED_WINDOWS_FLAC_SHA256")
    {
        Some(sha) => Some(("libFLAC.dll", sha)),
        None => None,
    };
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    const PACKAGED: Option<(&str, &str)> = None;

    let (filename, sha) = PACKAGED?;
    let executable = std::env::current_exe().ok();
    let path = resolve_library(resources, executable.as_deref(), filename)?;
    TrustedFlacRuntime::new(path, sha).ok()
}

/// Native launch/package paths only; no IPC accepts a dynamic-library path.
/// null_output is used by native desktop verification, never a UI setting.
pub fn configure_native_audio(
    client: MusicClient,
    resources: &Path,
    null_output: bool,
) -> MusicClient {
    let path = library(resources);
    let resources = resources.to_path_buf();
    client.with_audio_factory(move || {
        let engine = MpvEngine::open(MpvConfig {
            null_output,
            ..Default::default()
        })?;
        let engine = match &path {
            Some(path) => engine.with_effects_library(path.clone()),
            None => engine,
        };
        let engine = match packaged_flac(&resources) {
            Some(runtime) => engine.with_local_equalizer_decoder(runtime),
            None => engine,
        };
        Ok(Box::new(engine.with_local_equalizer_pcm_wave()))
    })
}

#[tauri::command]
pub async fn effect_catalogue(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    kind: CatalogueKind,
) -> Result<Vec<EffectDirectoryRow>, ClientError> {
    client
        .effect_catalogue(&epoch, kind)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn effect_directory_overview<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<EffectDirectoryOverview, ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "effect directory overview is main-window only",
        )
        .into());
    }
    client
        .effect_directory_overview(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn effect_detail(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    id: EffectId,
) -> Result<Option<EffectDirectoryDetail>, ClientError> {
    client.effect_detail(&epoch, id).await.map_err(Into::into)
}

#[tauri::command]
pub async fn effect_device_brands(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> Result<Vec<EffectDeviceRow>, ClientError> {
    client
        .effect_device_brands(&epoch)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn effect_device_models(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    brand: EffectId,
) -> Result<Vec<EffectDeviceRow>, ClientError> {
    client
        .effect_device_models(&epoch, brand)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn effect_device_material<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    brand: EffectId,
    model: EffectId,
) -> Result<DeviceMaterialCheck, ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput(
            "device material check requires the main window",
        )
        .into());
    }
    client
        .effect_device_material(&epoch, brand, model)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_filter_wins_over_a_separate_installation() {
        let temporary = tempfile::tempdir().unwrap();
        let portable = temporary.path().join("portable");
        let resources = temporary.path().join("installed");
        std::fs::create_dir_all(&portable).unwrap();
        std::fs::create_dir_all(resources.join("native")).unwrap();
        let filename = "synthetic-filter";
        let executable = portable.join("cumulus");
        let sibling = portable.join(filename);
        let bundled = resources.join("native").join(filename);
        std::fs::write(&bundled, b"installed version").unwrap();
        std::fs::write(&sibling, b"portable version").unwrap();
        assert_eq!(
            resolve_library(&resources, Some(&executable), filename),
            Some(sibling.clone())
        );
        std::fs::remove_file(sibling).unwrap();
        assert_eq!(
            resolve_library(&resources, Some(&executable), filename),
            Some(bundled)
        );
    }
}
