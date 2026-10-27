//! Native collection -> actual WebView, including caches and real page reload.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    identity::WindowsBrowserIdentity,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    watchman::*,
};
use cumulus_desktop::watchman::collection::{
    BrowserMeasurements, MeasurementBridge, MeasurementFrame,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{Manager, webview::PageLoadEvent};
struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/w/nuser/account/get");
            Ok(json!({"code":301}))
        })
    }
}
struct Counted {
    source: BrowserMeasurements<tauri::Wry>,
    calls: Mutex<Vec<String>>,
}
impl MeasurementSource for Counted {
    fn epoch(&self) -> Result<u64> {
        self.source.epoch()
    }
    fn initialize<'a>(&'a self, b: SdkBuild, c: &'a Config) -> ApiFuture<'a, ()> {
        self.source.initialize(b, c)
    }
    fn device<'a>(&'a self, key: &'a str, deferred: bool) -> ApiFuture<'a, MeasurementReading> {
        self.calls.lock().unwrap().push(format!("d:{key}"));
        self.source.device(key, deferred)
    }
    fn environment<'a>(
        &'a self,
        key: &'a str,
        t: [i64; 6],
        e: u64,
    ) -> ApiFuture<'a, EnvironmentReading> {
        self.calls.lock().unwrap().push(format!("e:{key}"));
        self.source.environment(key, t, e)
    }
    fn drain_events<'a>(&'a self, c: &'a Config, r: u64) -> ApiFuture<'a, Vec<Observation>> {
        self.calls.lock().unwrap().push("drain".into());
        self.source.drain_events(c, r)
    }
}
fn encoded(values: &[Observation]) -> Result<Vec<Value>> {
    let fields = descriptors()?;
    values.iter().map(|v|{
  let f=fields.iter().find(|f|f.key==v.key).ok_or(Error::State("probe descriptor missing"))?;
  let bytes=encode_field(f,&v.value,v.failed)?;
  Ok(json!({"key":v.key,"value":v.value,"failed":v.failed,"hex":bytes.iter().map(|b|format!("{b:02x}")).collect::<String>()}))
 }).collect()
}
async fn ready(bridge: &MeasurementBridge, old: Option<u64>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(epoch) = bridge.epoch()
                && old != Some(epoch)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| Error::State("actual measurement page did not become ready"))
}
async fn run(window: tauri::WebviewWindow, root: PathBuf) -> Result<()> {
    let bridge = window.state::<MeasurementBridge>().inner().clone();
    ready(&bridge, None).await?;
    let identity = WindowsBrowserIdentity::from_locales(&["C.UTF-8".into()], "zh-CN")?;
    let source = Arc::new(Counted {
        source: BrowserMeasurements::new(window.clone(), Some(identity))?,
        calls: Mutex::new(vec![]),
    });
    let collector = NativeCollection::new(source.clone());
    let mut reports = vec![];
    for build in [SdkBuild::Af2952a4, SdkBuild::B602a5ad7] {
        let config = Config::from_conf_for(
            build,
            "fixture",
            "",
            &encode_text(r#"{"ejcd":true,"ews":true}"#)?,
        )?;
        let before = source
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.starts_with("d:"))
            .count();
        let measured = collector
            .collect(CollectionMode::Device, &config, 0)
            .await?;
        if measured.device.len() != 38 {
            return Err(Error::State("actual device collection incomplete"));
        }
        for key in ["Jb", "Qa", "dc"] {
            if measured
                .device
                .iter()
                .find(|v| v.key == key)
                .is_none_or(|v| v.failed || v.value.as_str().is_none_or(|s| s.len() != 32))
            {
                return Err(Error::State("actual fingerprint unavailable"));
            }
        }
        if measured
            .device
            .iter()
            .find(|v| v.key == "La")
            .is_none_or(|v| v.failed || v.value != "")
        {
            return Err(Error::State("official audio outcome changed"));
        }
        let cached = collector
            .collect(CollectionMode::Device, &config, 0)
            .await?;
        if encoded(&cached.device)? != encoded(&measured.device)?
            || source
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.starts_with("d:"))
                .count()
                != before + 38
        {
            return Err(Error::State("actual device cache not reused"));
        }
        window.eval("(()=>{let b=document.getElementById('collection-probe-click');if(!b){b=document.createElement('button');b.id='collection-probe-click';document.body.append(b);}b.click();})();").map_err(|_|Error::State("probe input dispatch failed"))?;
        let behavior = collector
            .collect(CollectionMode::Behavior, &config, 123)
            .await?;
        if !behavior
            .events
            .iter()
            .any(|v| v.key == "_click" && v.value[2] == 3)
        {
            return Err(Error::State("programmatic input trust was not preserved"));
        }
        // Individual getter audit covers all 21, independently of ca's correct
        // early-abort behavior when the actual loader global is absent. Timing values
        // here are explicit test inputs; report collection above uses native timings.
        let groups: Value =
            serde_json::from_str(include_str!("../../core/src/watchman/collectors.json"))?;
        let mut individual = vec![];
        for key in groups["environment"].as_array().unwrap() {
            let key = key.as_str().unwrap();
            let observed = source.environment(key, [0; 6], 123).await?;
            let (value, failed) = match observed.reading {
                MeasurementReading::Value(v) => (v, false),
                MeasurementReading::Failed => (Value::Null, true),
                _ => return Err(Error::State("unexpected environment fingerprint")),
            };
            individual.extend(encoded(&[Observation {
                key: key.into(),
                value,
                failed,
            }])?);
        }
        reports.push(json!({"build":build.version(),"device":encoded(&measured.device)?,"cachedDevice":encoded(&cached.device)?,"deviceEnvironment":encoded(&measured.environment)?,"behaviorEnvironment":encoded(&behavior.environment)?,"events":encoded(&behavior.events)?,"individualEnvironment":individual,"individualTimingInputsAreSynthetic":true,"deviceCacheReused":true}));
    }
    let old = bridge.epoch()?;
    let (ticket, pending) = bridge.begin()?;
    window
        .reload()
        .map_err(|_| Error::State("actual reload dispatch failed"))?;
    let cancelled = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .map_err(|_| Error::State("reload did not cancel pending observation"))?
        .map_err(|_| Error::Cancelled)?;
    if !matches!(cancelled, Err(Error::StaleResponse)) {
        return Err(Error::State("reload pending result not stale"));
    }
    ready(&bridge, Some(old)).await?;
    if bridge
        .receive(MeasurementFrame {
            epoch: ticket.epoch,
            request: ticket.request,
            value: json!(false),
        })
        .is_ok()
    {
        return Err(Error::State("stale page frame accepted"));
    }
    let config = Config::from_conf_for(
        SdkBuild::B602a5ad7,
        "fixture",
        "",
        &encode_text(r#"{"ejcd":true,"ews":true}"#)?,
    )?;
    let reloaded = collector
        .collect(CollectionMode::Device, &config, 0)
        .await?;
    if reloaded.device.len() != 38
        || source
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.starts_with("d:"))
            .count()
            != 114
    {
        return Err(Error::State("reload reused stale device cache"));
    }
    fs::write(
        root.join("measurements.json"),
        serde_json::to_vec_pretty(
            &json!({"reports":reports,"afterReload":encoded(&reloaded.device)?,"sourceCalls":source.calls.lock().unwrap().clone()}),
        )?,
    )?;
    window.eval("(()=>{document.body.innerHTML='';const n=document.createElement('div');n.style.cssText='padding:32px;color:#222;background:white;font:20px sans-serif';n.textContent='真实原生采集验证通过：双版本设备缓存、环境字段、事件排空与页面重载。';document.body.append(n);})();").map_err(|_|Error::State("probe result display failed"))?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let summary = json!({"ok":true,"realWebview":true,"nativeCollectionConnected":true,"sdkBuilds":2,"deviceReads":114,"deviceFieldsPerBuild":38,"allEnvironmentGettersPerBuild":21,"actualPageReload":true,"stalePendingCancelled":true,"staleFrameRejected":true,"deviceCacheRebuiltAfterReload":true,"programmaticClickIsUntrusted":true,"networkRequests":0,"deviceReports":0,"behaviorReports":0,"musicRequestContextConnected":false});
    fs::write(
        root.join("complete.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    window.app_handle().exit(0);
    Ok(())
}
fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new private proof directory"));
    fs::create_dir(&root).unwrap();
    let setup_root = root.clone();
    let started = Arc::new(AtomicBool::new(false));
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&setup_root.join("state"), ApiOrigin::Pc)?;
            let session = Arc::new(AccountSession::new(
                Arc::new(Offline),
                Some(registry.store()),
            ));
            app.manage(MusicClient::with_session(session, registry));
            Ok(())
        })
        .on_page_load(move |webview, loaded| {
            if loaded.event() != PageLoadEvent::Finished || started.swap(true, Ordering::SeqCst) {
                return;
            }
            let root = root.clone();
            let window = webview.app_handle().get_webview_window("main").unwrap();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = run(window.clone(), root.clone()).await {
                    fs::write(
                        root.join("failed.json"),
                        json!({"error":error.to_string()}).to_string(),
                    )
                    .unwrap();
                    window.app_handle().exit(1);
                }
            });
        })
        .run(tauri::generate_context!())
        .expect("native collection proof");
}
