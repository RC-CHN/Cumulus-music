use serde_json::json;
use tauri::test::{INVOKE_KEY, assert_ipc_response, mock_builder, mock_context, noop_assets};

#[test]
fn invalid_copy_requests_are_rejected_before_accessing_the_os_clipboard() {
    let app = cumulus_desktop::configure(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    for text in [String::new(), "x".repeat(65537), "a\0b".into()] {
        assert_ipc_response(
            &window,
            tauri::webview::InvokeRequest {
                cmd: "copy_text".into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: if cfg!(windows) {
                    "http://tauri.localhost"
                } else {
                    "tauri://localhost"
                }
                .parse()
                .unwrap(),
                body: tauri::ipc::InvokeBody::Json(json!({"text":text})),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
            Err("无法复制空白或过长的文本".to_owned()),
        );
    }
}
