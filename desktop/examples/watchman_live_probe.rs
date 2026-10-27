//! Real native Watchman reports and a PC-policy-selected read of own playlist.
//! UI shell uses an offline account; all actual music requests are guarded here.
mod watchman_playlist_support;
mod watchman_write_support;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::MusicClient,
    identity::WindowsBrowserIdentity,
    protocol::{
        api::{Api, ApiFuture},
        context::business_for_path,
        http::*,
        mconfig::Update,
        wire::eapi,
    },
    storage::StateStore,
    watchman::*,
};
use cumulus_desktop::watchman::{collection::MeasurementBridge, host::NativeWatchman};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tauri::{Manager, webview::PageLoadEvent};
struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path != "/api/w/nuser/account/get" {
                return Err(Error::State("unexpected UI proof route"));
            }
            Ok(json!({"code":301}))
        })
    }
}
struct Reports {
    inner: NativeBackend,
    root: PathBuf,
    count: AtomicUsize,
}
impl Transport for Reports {
    fn send(&self, request: Request) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let n = self.count.fetch_add(1, Ordering::SeqCst);
            let details = match &request.kind {
                RequestKind::Configuration {
                    product, cache_key, ..
                } => json!({"product":product,"cacheKey":cache_key}),
                RequestKind::Report { build, packet, .. } => {
                    json!({"build":build.version(),"packet":packet})
                }
            };
            fs::write(
                self.root.join(format!("watchman-request-{n}.json")),
                serde_json::to_vec_pretty(
                    &json!({"host":request.host.name(),"path":request.path(),"details":details}),
                )?,
            )?;
            let result = self.inner.send(request).await;
            match &result {
                Ok(v) => fs::write(
                    self.root.join(format!("watchman-response-{n}.json")),
                    serde_json::to_vec_pretty(v)?,
                )?,
                Err(e) => fs::write(
                    self.root.join(format!("watchman-error-{n}.json")),
                    json!({"error":e.to_string()}).to_string(),
                )?,
            };
            result
        })
    }
}
struct Reads {
    inner: ReqwestBackend,
    root: PathBuf,
    count: AtomicUsize,
}
impl HttpBackend for Reads {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let params = url_params(&plan.body)?;
            let (path, payload) = eapi::unwrap(&params)?;
            if plan.url.host_str() != Some("interfacepc.music.163.com")
                || ![
                    "/api/w/nuser/account/get",
                    "/api/user/playlist",
                    "/api/v6/playlist/detail",
                ]
                .contains(&path.as_str())
            {
                return Err(Error::State("live proof only permits fixed reads"));
            }
            let token = plan.headers.get("X-antiCheatToken");
            if path == "/api/v6/playlist/detail"
                && (token.is_none() || payload["checkToken"].as_str() != token.map(String::as_str))
            {
                return Err(Error::State("protected read lacks matching native token"));
            }
            let n = self.count.fetch_add(1, Ordering::SeqCst);
            fs::write(
                self.root.join(format!("music-request-{n}.json")),
                serde_json::to_vec_pretty(
                    &json!({"path":path,"payload":payload,"mconfig":plan.headers.get("MConfig-Info"),"antiCheatToken":token}),
                )?,
            )?;
            let reply = self.inner.post(plan).await?;
            let body = serde_json::from_slice::<Value>(&reply.body)
                .or_else(|_| {
                    eapi::decrypt_response(&reply.body)
                        .map_err(|_| ())
                        .and_then(|v| serde_json::from_slice(&v).map_err(|_| ()))
                })
                .ok();
            fs::write(
                self.root.join(format!("music-response-{n}.json")),
                serde_json::to_vec_pretty(
                    &json!({"status":reply.status,"metadata":reply.metadata,"body":body}),
                )?,
            )?;
            Ok(reply)
        })
    }
}
// Form parser from std-only percent decoding: only the EAPI params field is read.
fn url_params(body: &[u8]) -> Result<String> {
    let s = std::str::from_utf8(body).map_err(|_| Error::State("non UTF-8 proof form"))?;
    let value = s
        .strip_prefix("params=")
        .ok_or(Error::State("unexpected proof form"))?;
    // EAPI encrypted hex never needs percent decoding.
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::State("unexpected EAPI form"));
    }
    Ok(value.into())
}
async fn run(
    window: tauri::WebviewWindow,
    root: PathBuf,
    profile: ClientProfile,
    reports: Arc<Reports>,
) -> Result<()> {
    let bridge = window.state::<MeasurementBridge>();
    tokio::time::timeout(Duration::from_secs(5), async {
        while bridge.epoch().is_err() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| Error::State("page not ready"))?;
    let host = window.state::<Arc<NativeWatchman>>().inner().clone();
    let path = PathBuf::from(
        std::env::var_os("CUMULUS_SESSION_DIR")
            .ok_or(Error::InvalidInput("explicit PC session required"))?,
    );
    if !path.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing PC session missing"));
    }
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(path)?),
        ApiOrigin::Pc,
    )?);
    let device = credentials.device_id()?;
    let reads = Arc::new(Reads {
        inner: ReqwestBackend::new(Duration::from_secs(20))?,
        root: root.clone(),
        count: AtomicUsize::new(0),
    });
    let api = Arc::new(MusicHttpClient::with_backend(
        credentials.clone(),
        profile.clone(),
        RequestOptions::default(),
        Some(host.context()),
        reads.clone(),
    )?);
    let account = api
        .call("/api/w/nuser/account/get", json!({}), false, &[200])
        .await?;
    let metadata = api
        .response_info()?
        .ok_or(Error::State("no account headers"))?
        .metadata;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if host.pc_config().resolve(&metadata).await? != Update::InProgress {
                return Ok::<_, Error>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| Error::State("PC config deadline"))??;
    let config = host.pc_config().snapshot()?;
    fs::write(
        root.join("pc-config.json"),
        serde_json::to_vec_pretty(&config)?,
    )?;
    let business = business_for_path(
        "/api/v6/playlist/detail",
        config
            .data
            .as_ref()
            .ok_or(Error::State("no actual PC configuration"))?,
        ApiOrigin::Pc.as_str(),
    )?
    .ok_or(Error::State(
        "actual PC config did not protect playlist read",
    ))?;
    let list = api
        .call(
            "/api/user/playlist",
            json!({"uid":account["profile"]["userId"],"includeVideo":true,"offset":0,"limit":30}),
            false,
            &[200],
        )
        .await?;
    let playlist = list["playlist"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|p| p["creator"]["userId"] == account["profile"]["userId"])
        })
        .ok_or(Error::State("no own playlist in first page"))?;
    let detail = api
        .call(
            "/api/v6/playlist/detail",
            json!({"id":playlist["id"],"n":100000,"s":0}),
            false,
            &[200],
        )
        .await?;
    if detail["playlist"]["id"] != playlist["id"] || credentials.device_id()? != device {
        return Err(Error::State("read identity changed"));
    }
    let playlist_audit = if let Some(original) = std::env::var_os("CUMULUS_PROOF_PLAYLIST_RECOVER")
    {
        Some(
            watchman_playlist_support::recover(
                &PathBuf::from(original),
                credentials.clone(),
                profile.clone(),
                host.context(),
            )
            .await?,
        )
    } else if std::env::var_os("CUMULUS_PROOF_PLAYLIST").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        Some(
            match watchman_playlist_support::exercise(
                &root,
                credentials.clone(),
                profile.clone(),
                host.context(),
            )
            .await
            {
                Ok(report) => report,
                Err(e) if root.join("playlist-plan.json").is_file() => {
                    fs::write(
                        root.join("playlist-run-error.json"),
                        serde_json::to_vec_pretty(&json!({"error":e.to_string()}))?,
                    )?;
                    let mut report = watchman_playlist_support::recover(
                        &root,
                        credentials.clone(),
                        profile.clone(),
                        host.context(),
                    )
                    .await?;
                    report["initialError"] = json!(e.to_string());
                    report
                }
                Err(e) => return Err(e),
            },
        )
    } else {
        None
    };
    let writes = if let Some(original) = std::env::var_os("CUMULUS_PROOF_WRITE_RECOVER") {
        Some(
            watchman_write_support::recover(
                &PathBuf::from(original),
                credentials.clone(),
                profile,
                host.context(),
            )
            .await?,
        )
    } else if std::env::var_os("CUMULUS_PROOF_WRITE").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        let outcome = watchman_write_support::exercise(
            &root,
            credentials.clone(),
            profile.clone(),
            host.context(),
        )
        .await;
        Some(match outcome {
            Ok(report) => report,
            Err(e) if root.join("write-plan.json").is_file() => {
                fs::write(
                    root.join("write-run-error.json"),
                    serde_json::to_vec_pretty(&json!({"error":e.to_string()}))?,
                )?;
                let mut recovered = watchman_write_support::recover(
                    &root,
                    credentials.clone(),
                    profile,
                    host.context(),
                )
                .await?;
                recovered["initialRunError"] = json!(e.to_string());
                recovered["recoveredAfterError"] = json!(true);
                recovered
            }
            Err(e) => return Err(e),
        })
    } else {
        None
    };
    if credentials.device_id()? != device {
        return Err(Error::State("write proof changed credential device ID"));
    }
    host.close()?;
    let summary = json!({"ok":true,"realWebview":true,"nativeCollectionConnected":true,"musicRequestContextConnected":true,"actualPcConfiguration":true,"pcConfigVersion":config.version,"protectedBusiness":business,"musicReads":reads.count.load(Ordering::SeqCst),"watchmanRequests":reports.count.load(Ordering::SeqCst),"protectedReadCode":detail["code"],"businessWrites":writes.as_ref().or(playlist_audit.as_ref()).and_then(|w|w["businessWrites"].as_u64()).unwrap_or(0),"writeAudit":writes,"playlistAudit":playlist_audit,"credentialDeviceIdUnchanged":true,"windowsDeclaration":true,"realWindowsEnvironment":false});
    let label = if summary["playlistAudit"]["recoveredOriginalJournal"] == true {
        "原歌单操作日志已回读核对，测试资源已清理；恢复过程未重放业务步骤。"
    } else if let Some(passed) = summary["playlistAudit"]["workflowPassed"].as_bool() {
        if passed {
            "官方 EAPI 歌单创建、编辑、增删歌曲、排序和删除已回读确认，原歌单列表已恢复。"
        } else {
            "官方 EAPI 歌单流程存在未通过步骤；已完成隔离资源清理，详见原始报告。"
        }
    } else {
        match summary["writeAudit"]["officialCreateAccepted"].as_bool() {
            Some(true) => "官方 EAPI 私密歌单创建已确认，清理后原歌单列表已恢复。",
            Some(false) => "官方 EAPI 私密歌单创建未通过；已完成回读审计，原歌单列表未变。",
            None => "PC 配置、设备／行为上报及本人歌单受保护读取已通过。",
        }
    };
    window.eval(format!("document.body.innerHTML='<div style=\"padding:32px;background:white;color:#222;font:20px sans-serif\">原生易盾实际请求验证：{label}</div>';"))
        .map_err(|_|Error::State("proof display failed"))?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    fs::write(
        root.join("complete.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    window.app_handle().exit(0);
    Ok(())
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof directory"));
    fs::create_dir(&root).unwrap();
    let setup = root.clone();
    let started = AtomicBool::new(false);
    let declaration =
        WindowsBrowserIdentity::from_locales(&[std::env::var("LANG").unwrap_or_default()], "zh-CN")
            .unwrap();
    let mut profile = declaration.native_profile().unwrap();
    #[cfg(target_os = "linux")]
    {
        profile = cumulus_core::identity::IdentityService::new(Arc::new(
            StateStore::open(root.join("identity")).unwrap(),
        ))
        .resolve(
            &cumulus_core::identity::collect_linux_identity(std::path::Path::new("/sys")).unwrap(),
        )
        .unwrap()
        .profile(profile)
        .unwrap();
    }
    let reports = Arc::new(Reports {
        inner: NativeBackend::new(&profile.user_agent).unwrap(),
        root: root.clone(),
        count: AtomicUsize::new(0),
    });
    let setup_reports = reports.clone();
    let setup_profile = profile.clone();
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry = ProfileRegistry::open(&setup.join("state"), ApiOrigin::Pc)?;
            app.manage(MusicClient::with_session(
                Arc::new(AccountSession::new(
                    Arc::new(Offline),
                    Some(registry.store()),
                )),
                registry.clone(),
            ));
            let window = app
                .get_webview_window("main")
                .ok_or("main window missing")?;
            app.manage(NativeWatchman::new(
                window,
                registry.store(),
                &setup_profile,
                Some(declaration),
                Some(setup_reports),
                None,
            )?);
            Ok(())
        })
        .on_page_load(move |webview, load| {
            if load.event() != PageLoadEvent::Finished || started.swap(true, Ordering::SeqCst) {
                return;
            }
            let root = root.clone();
            let window = webview.app_handle().get_webview_window("main").unwrap();
            let profile = profile.clone();
            let reports = reports.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = run(window.clone(), root.clone(), profile, reports).await {
                    if let Some(host) = window.try_state::<Arc<NativeWatchman>>() {
                        let _ = host.close();
                    }
                    fs::write(
                        root.join("failed.json"),
                        json!({"error":e.to_string()}).to_string(),
                    )
                    .unwrap();
                    window.app_handle().exit(1);
                }
            });
        })
        .run(tauri::generate_context!())
        .expect("live Watchman proof");
}
