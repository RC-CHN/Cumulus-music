use cumulus_core::{
    Error,
    protocol::{
        api::ApiFuture,
        http::ClientProfile,
        mconfig::{APPLICATION_KEY, ConfigBackend, ConfigUrl},
    },
    storage::StateStore,
    watchman::{Request, Transport},
};
use cumulus_desktop::watchman::{
    collection::{BrowserMeasurements, MeasurementBridge},
    host::NativeWatchman,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tauri::{
    Manager,
    test::{mock_builder, mock_context, noop_assets},
};
#[derive(Default)]
struct Pending(AtomicUsize);
impl Transport for Pending {
    fn send(&self, _: Request) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        })
    }
}
struct Config;
impl ConfigBackend for Config {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Ok(json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":["/api/read"]}]}}))
        })
    }
}

#[tokio::test]
async fn formal_host_single_start_close_cancels_native_tokens_and_releases_measurement_owner() {
    let app = cumulus_desktop::configure(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let backend = Arc::new(Pending::default());
    let host = NativeWatchman::new(
        window.clone(),
        store,
        &ClientProfile::default(),
        None,
        Some(backend.clone()),
        Some(Arc::new(Config)),
    )
    .unwrap();
    assert!(BrowserMeasurements::new(window.clone(), None).is_err());
    let bridge = window.state::<MeasurementBridge>();
    bridge.page_started().unwrap();
    bridge.page_finished().unwrap();
    host.pc_config().resolve(&BTreeMap::from([("MConfig-Info".into(),json!({APPLICATION_KEY:{"version":1,"os":"pc","fullUrl":"https://d5.music.126.net/obj/fixture/config.data"}}).to_string())])).await.unwrap();
    host.start().unwrap();
    host.start().unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while backend.0.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let context = host.context();
    let waiting = tokio::spawn(async move { context.prepare("/api/read", &json!({})).await });
    tokio::task::yield_now().await;
    host.close().unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::Cancelled)
    ));
    assert!(host.start().is_err());
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        host.pc_config().resolve(&BTreeMap::new()).await,
        Err(Error::Cancelled)
    ));
    drop(host);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if BrowserMeasurements::new(window.clone(), None).is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
