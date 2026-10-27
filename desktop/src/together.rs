use cumulus_core::client::{ClientError, MusicClient, dto::*};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager};
#[derive(Default)]
pub struct CloseCoordinator(pub Arc<AtomicBool>);
#[derive(Clone, serde::Serialize)]
struct CloseError {
    epoch: String,
    message: String,
}
pub fn closing<R: tauri::Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != "main" {
        return;
    }
    let Some(client) = window.try_state::<MusicClient>() else {
        return;
    };
    if !client.has_together_cleanup().unwrap_or(true) {
        return;
    }
    let epoch = client
        .account()
        .map(|account| account.epoch)
        .unwrap_or_default();
    api.prevent_close();
    let active = window.state::<CloseCoordinator>().0.clone();
    if active.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = window.app_handle().clone();
    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        let result = app.state::<MusicClient>().close_together().await;
        active.store(false, Ordering::Release);
        match result {
            Ok(()) => {
                let _ = window.destroy();
            }
            Err(error) => {
                let public = ClientError::from(error);
                let _ = window.emit(
                    "cumulus:together-close-error",
                    CloseError {
                        epoch,
                        message: format!(
                            "退出房间尚未完成：{}。窗口已保留，请核对后再关闭。",
                            public.message
                        ),
                    },
                );
            }
        }
    });
}
#[tauri::command]
pub async fn together_view(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<TogetherView>, ClientError> {
    client
        .together_view(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn together_command(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: TogetherCommand,
) -> Result<AccountData<TogetherView>, ClientError> {
    client
        .together_command(&epoch, command)
        .await
        .map_err(Into::into)
}
