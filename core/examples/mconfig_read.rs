//! Two fixed PC account reads with official M/92 negotiation; no writes/tokens.
use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    protocol::{
        api::{Api, ApiFuture},
        context::{RequestContext, TokenProvider},
        http::*,
        mconfig::*,
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
struct NoToken;
impl TokenProvider for NoToken {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async {
            Err(Error::State(
                "read became protected; explicit Watchman host needed",
            ))
        })
    }
}
struct Cdn {
    inner: NativeConfigBackend,
    root: PathBuf,
    count: AtomicUsize,
}
impl ConfigBackend for Cdn {
    fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let n = self.count.fetch_add(1, Ordering::SeqCst);
            fs::write(
                self.root.join(format!("config-request-{n}.json")),
                json!({"url":url.as_str()}).to_string(),
            )?;
            let data = self.inner.get(url).await?;
            fs::write(
                self.root.join(format!("config-response-{n}.json")),
                serde_json::to_vec_pretty(&data)?,
            )?;
            Ok(data)
        })
    }
}
struct Read {
    inner: ReqwestBackend,
    root: PathBuf,
    count: AtomicUsize,
}
impl HttpBackend for Read {
    fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            if request.url.as_str() != "https://interfacepc.music.163.com/eapi/w/nuser/account/get"
                || request.headers.contains_key("X-antiCheatToken")
            {
                return Err(Error::State("MConfig proof outside fixed read"));
            }
            let n = self.count.fetch_add(1, Ordering::SeqCst);
            fs::write(
                self.root.join(format!("request-{n}.json")),
                json!({"url":request.url.as_str(),"mconfig":request.headers.get("MConfig-Info")})
                    .to_string(),
            )?;
            let reply = self.inner.post(request).await?;
            fs::write(
                self.root.join(format!("response-{n}.json")),
                serde_json::to_vec_pretty(
                    &json!({"status":reply.status,"metadata":reply.metadata}),
                )?,
            )?;
            Ok(reply)
        })
    }
}
async fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "mconfig_read EXPLICIT_PC_PROFILE NEW_PRIVATE_ROOT",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing PC profile required"));
    }
    let root = PathBuf::from(&args[1]);
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root)?;
    let native = ClientProfile::windows_compatibility(
        cumulus_core::identity::WINDOWS_COMPATIBILITY_DESCRIPTION,
    )?;
    let cdn = Arc::new(Cdn {
        inner: NativeConfigBackend::new(&native.user_agent)?,
        root: root.clone(),
        count: AtomicUsize::new(0),
    });
    let state = Arc::new(StateStore::open(root.join("state"))?);
    let service = PcConfig::new(state, cdn.clone())?;
    let read = Arc::new(Read {
        inner: ReqwestBackend::new(Duration::from_secs(20))?,
        root: root.clone(),
        count: AtomicUsize::new(0),
    });
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let original_id = credentials.device_id()?;
    let context = Arc::new(RequestContext::from_pc(
        service.clone(),
        APP_VERSION,
        Arc::new(NoToken),
    )?);
    let client = MusicHttpClient::with_backend(
        credentials.clone(),
        native,
        RequestOptions::default(),
        Some(context),
        read.clone(),
    )?;
    let mut uid = None;
    let mut states = vec![];
    for _ in 0..2 {
        let response = client
            .request("/api/w/nuser/account/get", json!({}), false)
            .await?;
        if response["code"] != 200 {
            return Err(Error::Response("account read rejected"));
        }
        if let Some(previous) = &uid {
            if previous != &response["profile"]["userId"] {
                return Err(Error::State("account changed"));
            }
        } else {
            uid = Some(response["profile"]["userId"].clone());
        }
        let metadata = client
            .response_info()?
            .ok_or(Error::State("missing response metadata"))?
            .metadata;
        let update = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let result = service.resolve(&metadata).await?;
                if result != Update::InProgress {
                    return Ok::<_, Error>(result);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| Error::State("configuration update deadline"))??;
        states.push(json!({"result":update,"version":service.snapshot()?.version}));
    }
    let snapshot = service.snapshot()?;
    fs::write(
        root.join("snapshot.json"),
        serde_json::to_vec_pretty(&snapshot)?,
    )?;
    if credentials.device_id()? != original_id {
        return Err(Error::State("credential ID changed"));
    }
    let report = json!({"ok":true,"accountReads":read.count.load(Ordering::SeqCst),"cdnGets":cdn.count.load(Ordering::SeqCst),"pcConfigurationAvailable":snapshot.data.is_some(),"version":snapshot.version,"updates":states,"credentialDeviceIdUnchanged":true,"businessWrites":0,"watchmanReports":0});
    service.close()?;
    fs::write(
        root.join("complete.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("MConfig read: {e}");
        std::process::exit(1);
    }
}
