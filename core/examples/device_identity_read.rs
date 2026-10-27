//! Explicit PC account read with real local hardware context. No business writes.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    identity::*,
    protocol::{
        api::ApiFuture,
        http::{
            ApiOrigin, ClientProfile, HttpBackend, HttpReply, MusicHttpClient, RequestOptions,
            RequestPlan, ReqwestBackend,
        },
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct Guard {
    backend: ReqwestBackend,
    root: PathBuf,
    sign: String,
    device_id: String,
    user_agent: String,
    os_version: String,
    count: AtomicUsize,
}
impl HttpBackend for Guard {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let params = url::form_urlencoded::parse(&plan.body)
                .find(|(k, _)| k == "params")
                .ok_or(Error::State("expected EAPI read"))?
                .1
                .into_owned();
            let (path, payload) = eapi::unwrap(&params)?;
            let header: Value = serde_json::from_str(
                payload["header"]
                    .as_str()
                    .ok_or(Error::State("missing header"))?,
            )?;
            if path != "/api/w/nuser/account/get"
                || plan.url.scheme() != "https"
                || plan.url.host_str() != Some("interfacepc.music.163.com")
                || header["clientSign"] != self.sign
                || header["deviceId"] != self.device_id
                || header["osver"] != self.os_version
                || plan.headers.get("User-Agent") != Some(&self.user_agent)
            {
                return Err(Error::State("device proof scope or identity mismatch"));
            }
            let n = self.count.fetch_add(1, Ordering::SeqCst);
            // Explicit private evidence, excluding all Cookies and tokens.
            fs::write(
                self.root.join(format!("request-{n}.json")),
                serde_json::to_vec_pretty(
                    &json!({"path":path,"context":{"clientSign":header["clientSign"],"deviceId":header["deviceId"],"osver":header["osver"],"userAgent":self.user_agent}}),
                )?,
            )?;
            let response = self.backend.post(plan).await?;
            // Only code is needed; full account response is not duplicated here.
            let code = serde_json::from_slice::<Value>(&response.body)
                .ok()
                .map(|v| v["code"].clone());
            fs::write(
                self.root.join(format!("response-{n}.json")),
                serde_json::to_vec_pretty(&json!({"status":response.status,"code":code}))?,
            )?;
            Ok(response)
        })
    }
}
async fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(args.len() == 2 || (args.len() == 3 && args[2] == "--windows-compatible")) {
        return Err(Error::InvalidInput(
            "usage: device_identity_read EXPLICIT_PC_PROFILE NEW_PRIVATE_ROOT [--windows-compatible]",
        ));
    }
    if !cfg!(target_os = "linux") {
        return Err(Error::State(
            "local hardware collector is not implemented on this platform",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing PC profile required"));
    }
    let mut mkdir = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        mkdir.mode(0o700);
    }
    mkdir.create(&root)?;
    let facts = collect_linux_identity(std::path::Path::new("/sys"))?;
    let windows = args.len() == 3;
    let base_profile = if windows {
        ClientProfile::windows_compatibility(WINDOWS_COMPATIBILITY_DESCRIPTION)?
    } else {
        ClientProfile::default()
    };
    let identity =
        IdentityService::new(Arc::new(StateStore::open(root.join("identity"))?)).resolve(&facts)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(&profile)?),
        ApiOrigin::Pc,
    )?);
    let device_id = credentials.device_id()?;
    let guard = Arc::new(Guard {
        backend: ReqwestBackend::new(Duration::from_secs(20))?,
        root: root.clone(),
        sign: identity.client_sign()?,
        device_id: device_id.clone(),
        user_agent: base_profile.user_agent.clone(),
        os_version: base_profile.os_version.clone(),
        count: AtomicUsize::new(0),
    });
    let first_uid = {
        let api = Arc::new(MusicHttpClient::with_backend(
            credentials.clone(),
            identity.profile(base_profile.clone())?,
            RequestOptions::default(),
            None,
            guard.clone(),
        )?);
        let session =
            AccountSession::new(api, Some(Arc::new(StateStore::open(root.join("account"))?)));
        session.refresh().await?;
        session.token()?.uid
    };
    let reopened = IdentityService::new(Arc::new(StateStore::open(root.join("identity"))?))
        .resolve(&collect_linux_identity(std::path::Path::new("/sys"))?)?;
    if identity.client_sign()? != reopened.client_sign()? {
        return Err(Error::State("local identity changed during proof"));
    }
    let api = Arc::new(MusicHttpClient::with_backend(
        credentials.clone(),
        reopened.profile(base_profile)?,
        RequestOptions::default(),
        None,
        guard.clone(),
    )?);
    let session = AccountSession::new(api, Some(Arc::new(StateStore::open(root.join("account"))?)));
    session.refresh().await?;
    if first_uid.is_none()
        || session.token()?.uid != first_uid
        || credentials.device_id()? != device_id
    {
        return Err(Error::State("account or credential device changed"));
    }
    let report = json!({"ok":true,"summary":identity.summary(),"requests":guard.count.load(Ordering::SeqCst),"businessWrites":0,"identityStableAcrossReopen":true,"credentialDeviceIdUnchanged":true,"accountUnchanged":true,"declaredWindowsCompatibility":windows,"realWindowsEnvironment":false,"scope":"existing PC origin, no retry or protocol fallback"});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("device identity read proof: {e}");
        std::process::exit(1);
    }
}
