//! Isolated official CAPTCHA window. No application IPC or account cookies.
use cumulus_core::{Error, Result, client::LoginCaptchaProvider, protocol::api::ApiFuture};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tauri::{Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::oneshot;
mod page;

static SERIAL: AtomicU64 = AtomicU64::new(1);
type Completion = Arc<Mutex<Option<oneshot::Sender<Result<String>>>>>;

pub struct NativeCaptcha<R: Runtime>(pub tauri::AppHandle<R>);
struct WindowGuard<R: Runtime> {
    window: WebviewWindow<R>,
    _directory: tempfile::TempDir,
}
impl<R: Runtime> Drop for WindowGuard<R> {
    fn drop(&mut self) {
        let _ = self.window.destroy();
    }
}
fn finish(slot: &Completion, result: Result<String>) {
    if let Ok(mut slot) = slot.lock()
        && let Some(sender) = slot.take()
    {
        let _ = sender.send(result);
    }
}
impl<R: Runtime> LoginCaptchaProvider for NativeCaptcha<R> {
    fn solve(&self) -> ApiFuture<'_, String> {
        Box::pin(async move {
            let (sender, receiver) = oneshot::channel();
            let completion: Completion = Arc::new(Mutex::new(Some(sender)));
            let nav = completion.clone();
            // Tauri/Wry deduplicates Linux protocol registration by context
            // directory, while incognito creates a new WebKit context. A
            // distinct directory prevents the ephemeral view losing handlers.
            let directory = tempfile::Builder::new()
                .prefix("cumulus-captcha-")
                .tempdir()?;
            let label = format!("captcha-{}", SERIAL.fetch_add(1, Ordering::Relaxed));
            let window = WebviewWindowBuilder::new(
                &self.0,
                label,
                WebviewUrl::CustomProtocol(
                    "cumulus-captcha://localhost/"
                        .parse()
                        .map_err(|_| Error::State("CAPTCHA unavailable"))?,
                ),
            )
            .title("网易云音乐安全验证 — Cumulus")
            .inner_size(440.0, 520.0)
            .center()
            .resizable(false)
            .incognito(true)
            .data_directory(directory.path().to_path_buf())
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .on_navigation(move |url| {
                if !page::local(url) {
                    return false;
                }
                if url.path() == "/result" {
                    finish(&nav, page::result(url));
                    return false;
                }
                url.path() == "/" && url.query().is_none() && url.fragment().is_none()
            })
            .build()
            .map_err(|_| Error::State("CAPTCHA unavailable"))?;
            let guard = WindowGuard {
                window: window.clone(),
                _directory: directory,
            };
            window.on_window_event(move |event| {
                if matches!(
                    event,
                    tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                ) {
                    finish(&completion, Err(Error::Cancelled));
                }
            });
            let result = tokio::time::timeout(std::time::Duration::from_secs(60), receiver)
                .await
                .map_err(|_| Error::State("CAPTCHA timed out"))?
                .map_err(|_| Error::Cancelled)?;
            drop(guard);
            result
        })
    }
}

pub fn protocol<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_uri_scheme_protocol("cumulus-captcha", |context, request| {
        page::response(context.webview_label(), &request)
    })
}

#[tauri::command]
pub async fn solve_login_captcha<R: Runtime>(
    window: WebviewWindow<R>,
    client: tauri::State<'_, cumulus_core::client::MusicClient>,
    operation_id: String,
) -> std::result::Result<
    cumulus_core::client::dto::LoginCaptchaView,
    cumulus_core::client::ClientError,
> {
    let result = client
        .solve_login_captcha(&operation_id, &NativeCaptcha(window.app_handle().clone()))
        .await;
    crate::auth::publish(&window, &client);
    result.map_err(Into::into)
}
