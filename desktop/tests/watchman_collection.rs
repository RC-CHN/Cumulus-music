use cumulus_core::Error;
use cumulus_desktop::watchman::collection::{
    BrowserMeasurements, MeasurementBridge, MeasurementFrame,
};
use serde_json::{Value, json};
use tauri::{
    Manager,
    test::{INVOKE_KEY, assert_ipc_response, mock_builder, mock_context, noop_assets},
};

#[tokio::test]
async fn measurement_tickets_are_bounded_cancelled_on_reload_and_never_reused() {
    let bridge = MeasurementBridge::default();
    assert!(bridge.begin().is_err());
    bridge.page_started().unwrap();
    bridge.page_finished().unwrap();
    let (old, old_result) = bridge.begin().unwrap();
    bridge.page_started().unwrap();
    assert!(matches!(
        old_result.await.unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(
        bridge
            .receive(MeasurementFrame {
                epoch: old.epoch,
                request: old.request,
                value: json!(1)
            })
            .is_err()
    );
    bridge.page_finished().unwrap();
    let (new, result) = bridge.begin().unwrap();
    assert_ne!(old.request, new.request);
    bridge
        .receive(MeasurementFrame {
            epoch: new.epoch,
            request: new.request,
            value: json!({"value":7,"failed":false}),
        })
        .unwrap();
    assert_eq!(result.await.unwrap().unwrap()["value"], 7);
    assert!(
        bridge
            .receive(MeasurementFrame {
                epoch: new.epoch,
                request: new.request,
                value: json!(2)
            })
            .is_err()
    );
    let mut tickets = Vec::new();
    for _ in 0..8 {
        tickets.push(bridge.begin().unwrap());
    }
    assert!(bridge.begin().is_err());
    bridge.cancel(tickets[0].0);
    let (last, _) = bridge.begin().unwrap();
    bridge.cancel(last);
    bridge.page_started().unwrap();
    assert!(bridge.epoch().is_err());
}

#[test]
fn registered_measurement_ipc_and_source_ownership_stay_in_the_main_window() {
    let app = cumulus_desktop::configure(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let other = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .unwrap();
    let source = BrowserMeasurements::new(window.clone(), None).unwrap();
    assert!(BrowserMeasurements::new(window.clone(), None).is_err());
    assert!(BrowserMeasurements::new(other.clone(), None).is_err());
    let bridge = app.state::<MeasurementBridge>();
    bridge.page_started().unwrap();
    bridge.page_finished().unwrap();
    let (ticket, _reply) = bridge.begin().unwrap();
    let request = |v: Value| tauri::webview::InvokeRequest {
        cmd: "watchman_measurement".into(),
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
    let frame = json!({"epoch":ticket.epoch,"request":ticket.request,"value":false});
    assert_ipc_response(
        &other,
        request(frame.clone()),
        Err("Application commands require the main window".to_owned()),
    );
    assert_ipc_response(&window, request(frame.clone()), Ok(json!(null)));
    assert_ipc_response(
        &window,
        request(frame),
        Err("watchman_measurement_rejected".to_owned()),
    );
    drop(source);
    assert!(BrowserMeasurements::new(window, None).is_ok());
}
