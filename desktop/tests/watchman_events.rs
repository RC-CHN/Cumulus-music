use cumulus_core::watchman::{Config, encode_text};
use cumulus_desktop::watchman::{EventFrame, EventHost};
use serde_json::{Value, json};
use tauri::{
    Manager,
    test::{INVOKE_KEY, assert_ipc_response, mock_builder, mock_context, noop_assets},
};
fn config() -> Config {
    Config::from_conf("testProduct", "", &encode_text("{}").unwrap()).unwrap()
}
fn frame(generation: u64, sequence: u64) -> Value {
    json!({"generation":generation,"sequence":sequence,"observed_ms":1100,"payload":{"kind":"event","sample":{"dom_type":"click","trusted":true,"target_id":"native-test"}}})
}
#[test]
fn native_event_scope_clears_on_restart_and_faults_on_sequence_loss() {
    let host = EventHost::default();
    let a = host.begin(&config(), true, 1000).unwrap();
    host.receive(serde_json::from_value(frame(a, 1)).unwrap())
        .unwrap();
    let b = host.begin(&config(), true, 1000).unwrap();
    assert_ne!(a, b);
    assert!(
        host.receive(serde_json::from_value(frame(a, 2)).unwrap())
            .is_err()
    );
    assert!(host.drain(b, 1200).unwrap().is_empty());
    assert!(
        host.receive(serde_json::from_value(frame(b, 2)).unwrap())
            .is_err()
    );
    assert!(host.drain(b, 1300).is_err());
    let c = host.begin(&config(), true, 2000).unwrap();
    let overflow: EventFrame = serde_json::from_value(
        json!({"generation":c,"sequence":1,"observed_ms":2100,"payload":{"kind":"overflow"}}),
    )
    .unwrap();
    assert!(host.receive(overflow).is_err());
    assert!(host.drain(c, 2200).is_err());
    host.stop().unwrap();
    assert!(host.received(c).is_err());
}
#[test]
fn real_registered_event_ipc_is_main_window_only_and_inactive_by_default() {
    let app = cumulus_desktop::configure(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let other = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .unwrap();
    let request = |v: Value| tauri::webview::InvokeRequest {
        cmd: "watchman_observation".into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: tauri::ipc::InvokeBody::Json(json!({"frame":v})),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    };
    assert_ipc_response(
        &window,
        request(frame(1, 1)),
        Err("watchman_observation_rejected".to_owned()),
    );
    let host = app.state::<EventHost>();
    let generation = host.begin(&config(), true, 1000).unwrap();
    assert_ipc_response(
        &other,
        request(frame(generation, 1)),
        Err("Application commands require the main window".to_owned()),
    );
    assert_ipc_response(&window, request(frame(generation, 1)), Ok(json!(null)));
    assert_eq!(host.received(generation).unwrap(), 1);
    let values = host.drain(generation, 1200).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].value[2], 2);
    assert_eq!(values[0].value[5], "native-test");
}
