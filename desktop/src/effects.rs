use cumulus_core::{
    client::{ClientError, MusicClient},
    effects::{
        catalog::{CatalogueKind, EffectId},
        device_directory::EffectDeviceRow,
        directory::{EffectDirectoryDetail, EffectDirectoryRow},
    },
    playback::engine::{MpvConfig, MpvEngine},
};
use std::path::{Path, PathBuf};

fn library(resources: &Path) -> Option<PathBuf> {
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
    let bundled = resources.join("native").join(filename);
    if bundled.is_file() {
        return Some(bundled);
    }
    let sibling = std::env::current_exe().ok()?.parent()?.join(filename);
    sibling.is_file().then_some(sibling)
}
/// Native launch/package paths only; no IPC accepts a dynamic-library path.
/// null_output is used by native desktop verification, never a UI setting.
pub fn configure_native_audio(
    client: MusicClient,
    resources: &Path,
    null_output: bool,
) -> MusicClient {
    let path = library(resources);
    client.with_audio_factory(move || {
        let engine = MpvEngine::open(MpvConfig {
            null_output,
            ..Default::default()
        })?;
        Ok(Box::new(match &path {
            Some(path) => engine.with_effects_library(path.clone()),
            None => engine,
        }))
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
