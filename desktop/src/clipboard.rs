//! Plain-text copy only, routed through the main window's typed IPC boundary.
use tauri_plugin_clipboard_manager::ClipboardExt;

#[tauri::command]
pub async fn copy_text<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    text: String,
) -> Result<(), String> {
    if text.is_empty() || text.len() > 64 * 1024 || text.contains('\0') {
        return Err("无法复制空白或过长的文本".into());
    }
    app.clipboard()
        .write_text(text)
        .map_err(|_| "复制失败，请稍后再试".into())
}
