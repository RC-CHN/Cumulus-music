use cumulus_core::client::{ClientError, MusicClient, dto::*};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};

fn search_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<(), ClientError> {
    if window.label() != "main" {
        return Err(cumulus_core::Error::InvalidInput("search is main-window only").into());
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct RightsPump(AtomicBool);

fn observe<R: tauri::Runtime>(app: &tauri::AppHandle<R>, client: &MusicClient) {
    let pump = app.state::<RightsPump>();
    if pump.0.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut source) = client.subscribe_membership_observations() else {
        return;
    };
    if pump.0.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while source.changed().await.is_ok() {
            let observation = source.borrow_and_update().clone();
            if let Some(event) = observation {
                let _ = app.emit_to("main", "cumulus:search-privilege-changed", event);
            }
        }
    });
}

#[tauri::command]
pub async fn search_results<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: SearchResultQuery,
    default_receipt: Option<String>,
    correction_receipt: Option<String>,
) -> Result<AccountData<SearchResultView>, ClientError> {
    search_window(&window)?;
    observe(window.app_handle(), &client);
    client
        .search_results_with_context(
            &epoch,
            query,
            default_receipt.as_deref(),
            correction_receipt.as_deref(),
        )
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn search_group_download<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: SearchGroupDownloadCommand,
) -> Result<SearchGroupDownloadReply, ClientError> {
    search_window(&window)?;
    client
        .search_group_download(&epoch, command)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_refresh_privileges(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    receipt: String,
    observation: Option<String>,
) -> Result<AccountData<SearchPrivilegePatch>, ClientError> {
    client
        .search_refresh_privileges(&epoch, &receipt, observation.as_deref())
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_voice_menu<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: SearchVoiceMenuTarget,
) -> Result<AccountData<SearchVoiceMenu>, ClientError> {
    search_window(&window)?;
    client
        .search_voice_menu(
            &epoch,
            target,
            crate::network_diagnostics::observed_default_route(&window),
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_open_voice_directory<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: SearchVoiceMenuTarget,
) -> Result<(), ClientError> {
    search_window(&window)?;
    let path = client.search_voice_directory_path(&epoch, target).await?;
    client.validate_download_epoch(&epoch)?;
    crate::file_manager::open_directory(path)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_reason_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    target: ReasonTarget,
) -> Result<AccountData<SearchReasonPage>, ClientError> {
    client
        .search_reason_page(&epoch, target)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn search_discovery<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: SearchDiscoveryQuery,
) -> Result<AccountData<SearchDiscovery>, ClientError> {
    search_window(&window)?;
    client
        .search_discovery(&epoch, query)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub fn search_history<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    command: SearchHistoryCommand,
) -> Result<AccountData<Vec<String>>, ClientError> {
    search_window(&window)?;
    client.search_history(&epoch, command).map_err(Into::into)
}
