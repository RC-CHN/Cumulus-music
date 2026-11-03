//! Explicit human-authorized QR login proof. Never run by CI. Only sanitized
//! status/credential-presence metadata is printed; secrets stay in native stores.
use cumulus_core::{
    Error, Result,
    account::{
        AccountSession, QrStatus,
        profiles::ProfileRegistry,
        refresh::{AccountLifecycle, DesktopDeviceToken, DeviceInfo},
    },
    client::{ClientError, MusicClient, dto::RenewalAction},
    identity::WINDOWS_COMPATIBILITY_DESCRIPTION,
    protocol::{
        api::ApiFuture,
        http::{
            ApiOrigin, ClientProfile, HttpBackend, HttpReply, MusicHttpClient, RequestOptions,
            RequestPlan, ReqwestBackend,
        },
        wire::eapi,
    },
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const IDENTITY_MODE: &str = if cfg!(target_os = "linux") {
    "windows-local"
} else {
    "windows-compatibility"
};

struct ObservedHttp {
    inner: ReqwestBackend,
    events: Mutex<Vec<serde_json::Value>>,
    check_existing: bool,
    refresh_sent: std::sync::atomic::AtomicBool,
}
impl HttpBackend for ObservedHttp {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            let (path, payload) =
                eapi::unwrap(form.get("params").ok_or(Error::Response("expected EAPI"))?)?;
            let allowed = matches!(
                path.as_str(),
                "/api/rtrs/abt/front/expinfo/list" | "/api/w/nuser/account/get"
            ) || (!self.check_existing
                && matches!(
                    path.as_str(),
                    "/api/login/qrcode/unikey" | "/api/login/qrcode/client/login"
                ))
                || (self.check_existing && path == "/api/login/token/refresh/v1");
            if !allowed {
                return Err(Error::RequestNotSent(Box::new(Error::State(
                    "QR proof endpoint denied",
                ))));
            }
            if path == "/api/login/token/refresh/v1"
                && self
                    .refresh_sent
                    .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(Error::RequestNotSent(Box::new(Error::State(
                    "proof permits only one refresh request",
                ))));
            }
            let reply = self.inner.post(plan).await?;
            let decoded = serde_json::from_slice::<serde_json::Value>(&reply.body)
                .ok()
                .or_else(|| {
                    eapi::decrypt_response(&reply.body)
                        .ok()
                        .and_then(|v| serde_json::from_slice(&v).ok())
                });
            let assignments = if path == "/api/rtrs/abt/front/expinfo/list" {
                decoded.as_ref().and_then(|v| {
                    cumulus_core::account::login_policy::LoginExperiments::from_response(v).ok()
                })
            } else {
                None
            };
            let event = json!({"path":path,"qrType":payload.get("type"),"jsk":payload["fromPage"] == "JSK","httpStatus":reply.status,"responseCode":decoded.as_ref().and_then(|v|v["code"].as_i64()),"experiments":assignments,"bodyStatus":format!("{:?}", reply.body_status),"refreshHeaderPresent":reply.refresh_token.as_ref().is_some_and(|v|!v.is_empty()),"musicUHeaderPresent":reply.set_cookies.iter().any(|s|s.starts_with("MUSIC_U=") && !s.starts_with("MUSIC_U=;"))});
            self.events
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .push(event);
            Ok(reply)
        })
    }
}

fn save(root: &Path, name: &str, value: serde_json::Value) -> Result<()> {
    std::fs::write(root.join(name), serde_json::to_vec_pretty(&value)?)?;
    Ok(())
}

fn native_profile(root: &Path) -> Result<ClientProfile> {
    let base = ClientProfile::windows_compatibility(WINDOWS_COMPATIBILITY_DESCRIPTION)?;
    #[cfg(target_os = "linux")]
    {
        use cumulus_core::{
            identity::{IdentityService, collect_linux_identity},
            storage::StateStore,
        };
        let facts = collect_linux_identity(Path::new("/sys"))?;
        IdentityService::new(Arc::new(StateStore::open(root.join("identity"))?))
            .resolve(&facts)?
            .profile(base)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Ok(base)
    }
}

fn device() -> DeviceInfo {
    #[cfg(target_os = "linux")]
    let model = std::fs::read_to_string("/sys/class/dmi/id/product_name")
        .ok()
        .filter(|s| s.len() <= 1024 && !s.contains('\0'))
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    #[cfg(not(target_os = "linux"))]
    let model = String::new();
    DeviceInfo {
        device_name: model,
        os: "pc".into(),
        client_type: Some("pc".into()),
        ..Default::default()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!(
            "{}",
            serde_json::to_string(&ClientError::from(error)).expect("static error serialization")
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or(Error::InvalidInput(
        "explicit isolated data directory required",
    ))?);
    let (check_existing, rotate_once) = match args.next() {
        None => (false, false),
        Some(value) if value == "--check-existing" => (true, false),
        Some(value) if value == "--rotate-once" => (true, true),
        _ => return Err(Error::InvalidInput("unknown proof mode")),
    };
    if args.next().is_some() {
        return Err(Error::InvalidInput("unexpected proof argument"));
    }
    if !check_existing && root.exists() {
        return Err(Error::InvalidInput("proof requires a new directory"));
    }
    if check_existing && !root.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("missing proof registry"));
    }
    if !check_existing {
        std::fs::create_dir_all(&root)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    // Same explicit windows-local native identity option as the desktop host.
    // This core-only proof does not claim a WebView guardian token acquisition.
    let profile = native_profile(&root)?;
    let http = Arc::new(ObservedHttp {
        inner: ReqwestBackend::new(Duration::from_secs(20))?,
        events: Mutex::new(vec![]),
        check_existing,
        refresh_sent: std::sync::atomic::AtomicBool::new(false),
    });
    if rotate_once {
        // Explicit diagnostic of the SDK primitive, not the desktop AB gate.
        // Normal daily/pending guards remain active; unknown results never replay.
        let proof: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("status.json"))?)?;
        if proof["status"] != "authorized" || proof["implementation"] != "rust-core" {
            return Err(Error::InvalidInput(
                "rotation requires this probe's isolated login",
            ));
        }
        // Reserve across processes before any diagnostic network I/O. A failed
        // run keeps this marker, so restarting the tool cannot replay a write.
        {
            use std::io::Write;
            let mut marker = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("rotation-status.json"))?;
            marker.write_all(br#"{"status":"started","replayAllowed":false}"#)?;
        }
        let registry = ProfileRegistry::open(&root, ApiOrigin::Pc)?;
        let selected = registry
            .current(profile.clone())?
            .ok_or(Error::State("no saved account"))?;
        let before = selected.credentials.login_credentials()?;
        let native = Arc::new(MusicHttpClient::with_backend(
            selected.credentials.clone(),
            profile.clone(),
            RequestOptions::default(),
            None,
            http.clone(),
        )?);
        let session = Arc::new(AccountSession::new(native.clone(), None));
        session.refresh().await?;
        let mut device = device();
        device.device_id = selected.credentials.device_id()?;
        let lifecycle = AccountLifecycle::new(session, native);
        let result = lifecycle
            .refresh_jsk(&device, &DesktopDeviceToken, false)
            .await;
        let after = registry
            .current(profile)?
            .ok_or(Error::State("no saved account"))?
            .credentials
            .login_credentials()?;
        let events = http.events.lock().map_err(|_| Error::LockPoisoned)?;
        save(&root, "rotation-http-events.json", json!(*events))?;
        let service_code = result.as_ref().err().and_then(|error| match error.root() {
            Error::Service { code, .. } => code.as_i64(),
            _ => None,
        });
        let report = json!({"status":"finished","mode":"explicit-sdk-primitive","automaticPolicyEnabled":false,"forcedDailyGate":false,"outcome":result.as_ref().ok(),"serviceCode":service_code,"error":result.err().map(ClientError::from),"musicUPersisted":after.token.as_ref().is_some_and(|s|!s.is_empty()),"refreshTokenPersisted":after.refresh_token.as_ref().is_some_and(|s|!s.is_empty()),"musicUChanged":before.token != after.token,"refreshTokenChanged":before.refresh_token != after.refresh_token,"rotationReceiptChanged":before.rotation != after.rotation,"refreshRequests":events.iter().filter(|v|v["path"] == "/api/login/token/refresh/v1").count(),"journal":lifecycle.refresh_status()?,"businessMutations":0,"guardianWebView":false});
        save(&root, "rotation-status.json", report.clone())?;
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    let client =
        MusicClient::open_with_native_profile(&root, None, profile.clone(), Some(http.clone()))?
            .with_official_login_policy()
            .with_session_renewal(device(), Arc::new(DesktopDeviceToken));
    if check_existing {
        let registry = ProfileRegistry::open(&root, ApiOrigin::Pc)?;
        let before = registry
            .current(profile.clone())?
            .ok_or(Error::State("no saved account"))?
            .credentials
            .login_credentials()?;
        let account = client.restore_account().await?;
        let result = client
            .renew_session(&account.epoch, RenewalAction::Check)
            .await;
        let after = registry
            .current(profile)?
            .ok_or(Error::State("no saved account"))?
            .credentials
            .login_credentials()?;
        let events = http.events.lock().map_err(|_| Error::LockPoisoned)?;
        save(&root, "renewal-http-events.json", json!(*events))?;
        let report = json!({"status":"reopened","accountStatus":client.account()?.status,"musicUPersisted":after.token.as_ref().is_some_and(|s|!s.is_empty()),"refreshTokenPersisted":after.refresh_token.as_ref().is_some_and(|s|!s.is_empty()),"musicUChanged":before.token != after.token,"refreshTokenChanged":before.refresh_token != after.refresh_token,"renewal":result.as_ref().ok(),"error":result.err().map(ClientError::from),"refreshRequests":events.iter().filter(|v|v["path"] == "/api/login/token/refresh/v1").count(),"businessMutations":0,"officialPolicy":true,"forced":false,"guardianWebView":false});
        save(&root, "renewal-status.json", report.clone())?;
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    let view = client.begin_qr().await?;
    let side = view.modules.len();
    let scale = 8;
    let image = image::GrayImage::from_fn(
        ((side + 8) * scale) as u32,
        ((side + 8) * scale) as u32,
        |x, y| {
            let (x, y) = (x as usize / scale, y as usize / scale);
            image::Luma([
                if x >= 4 && y >= 4 && x < side + 4 && y < side + 4 && view.modules[y - 4][x - 4] {
                    0
                } else {
                    255
                },
            ])
        },
    );
    image
        .save(root.join("login-qr.png"))
        .map_err(|_| Error::State("QR image write failed"))?;
    save(
        &root,
        "status.json",
        json!({"status":"waiting_scan","implementation":"rust-core","identity":IDENTITY_MODE,"officialPolicy":true,"guardianWebView":false}),
    )?;
    println!("QR_READY {}", root.join("login-qr.png").display());
    let started = Instant::now();
    let mut prior = QrStatus::WaitingScan;
    while started.elapsed() < Duration::from_secs(300) {
        tokio::time::sleep(Duration::from_millis(view.poll_after_ms.into())).await;
        let status = match client.poll_qr(&view.operation_id).await {
            Ok(status) => status,
            Err(_) => {
                save(
                    &root,
                    "http-events.json",
                    json!(*http.events.lock().map_err(|_| Error::LockPoisoned)?),
                )?;
                save(
                    &root,
                    "status.json",
                    json!({"status":"request_failed","secretsPrinted":false}),
                )?;
                println!("QR_REQUEST_FAILED");
                return Ok(());
            }
        };
        if status != prior {
            println!("QR_STATUS {status:?}");
            prior = status;
        }
        save(
            &root,
            "http-events.json",
            json!(*http.events.lock().map_err(|_| Error::LockPoisoned)?),
        )?;
        if status == QrStatus::Authorized {
            let selected = ProfileRegistry::open(&root, ApiOrigin::Pc)?
                .current(profile)?
                .ok_or(Error::State("no selected profile after login"))?;
            let stored = selected.credentials.login_credentials()?;
            let account = client.account()?;
            let report = json!({"status":"authorized","accountStatus":account.status,"musicUPersisted":stored.token.as_ref().is_some_and(|s|!s.is_empty()),"refreshTokenPersisted":stored.refresh_token.as_ref().is_some_and(|s|!s.is_empty()),"loginTimestampPersisted":stored.last_login_at.is_some(),"renewal":client.session_renewal(&account.epoch)?,"rotationAttempted":false,"businessMutations":0,"implementation":"rust-core","identity":IDENTITY_MODE,"officialPolicy":true,"guardianWebView":false});
            save(&root, "status.json", report.clone())?;
            println!("{}", serde_json::to_string(&report)?);
            return Ok(());
        }
        save(&root, "status.json", json!({"status":status}))?;
        if matches!(
            status,
            QrStatus::Expired | QrStatus::Challenge | QrStatus::Error | QrStatus::Cancelled
        ) {
            return Ok(());
        }
    }
    client.cancel_qr(&view.operation_id)?;
    save(&root, "status.json", json!({"status":"timed_out"}))?;
    println!("QR_TIMEOUT");
    Ok(())
}
