use cumulus_core::{
    Error,
    client::{ClientError, MusicClient, dto::*},
};
#[tauri::command]
pub async fn member_rights(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    refresh: bool,
) -> Result<AccountData<MemberRightsView>, ClientError> {
    client
        .member_rights(&epoch, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn member_page(
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    query: MemberQuery,
    refresh: bool,
) -> Result<AccountData<MemberPageView>, ClientError> {
    client
        .member_page(&epoch, query, refresh)
        .await
        .map_err(Into::into)
}
#[tauri::command]
pub async fn open_member_link<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    epoch: String,
    revision: String,
    key: String,
) -> Result<(), ClientError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || -> cumulus_core::Result<()> {
        let client = app.state::<MusicClient>();
        let url = client.member_external_url(&epoch, &revision, &key)?;
        #[cfg(target_os = "linux")]
        let mut command = std::process::Command::new("xdg-open");
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut c = std::process::Command::new("/usr/bin/open");
            c.arg("--");
            c
        };
        #[cfg(windows)]
        let mut command = std::process::Command::new("explorer.exe");
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        return Err(Error::State("browser opening unavailable"));
        #[cfg(any(target_os = "linux", target_os = "macos", windows))]
        {
            let status = command
                .arg(url)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if !status.success() {
                return Err(Error::State("browser failed to open"));
            }
            Ok(())
        }
    })
    .await
    .map_err(|_| ClientError::from(Error::State("browser worker stopped")))?
    .map_err(Into::into)
}
