//! Typed floor pagination/cache proof. Explicit account, read-only whitelist.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    comments::*,
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
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
};
struct Reads {
    api: Arc<dyn Api>,
    root: PathBuf,
    count: AtomicUsize,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get" | "/api/resource/comment/floor/get"
            ) {
                return Err(Error::State("write outside floor read proof"));
            }
            let n = self.count.fetch_add(1, Ordering::SeqCst);
            let value = self.api.request(path, payload.clone(), cache).await?;
            fs::write(
                self.root.join(format!("api-{n:03}.json")),
                serde_json::to_vec_pretty(
                    &json!({"path":path,"payload":payload,"response":value}),
                )?,
            )?;
            Ok(value)
        })
    }
}
async fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(Error::InvalidInput(
            "usage: comment_floor_roundtrip PC_PROFILE NEW_PRIVATE_ROOT TRACK_ID COMMENT_ID",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing PC profile required"));
    }
    let resource = CommentResource {
        kind: ResourceKind::Track,
        id: identifier(&json!(args[2]))?,
        owner: None,
    };
    let query = CommentFloorQuery::first(identifier(&json!(args[3]))?);
    fs::create_dir(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    }
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let reads = Arc::new(Reads {
        api: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        root: root.clone(),
        count: AtomicUsize::new(0),
    });
    let store = Arc::new(StateStore::open(root.join("state"))?);
    let session = Arc::new(AccountSession::new(reads.clone(), Some(store)));
    session.refresh().await?;
    let token = session.token()?;
    let service = Comments::new(session)?;
    let first = service.floor_for(&token, &resource, &query).await?;
    let second = if let Some(next) = &first.next {
        Some(service.floor_for(&token, &resource, next).await?)
    } else {
        None
    };
    let mut ids = std::collections::BTreeSet::new();
    for c in first
        .comments
        .iter()
        .chain(second.iter().flat_map(|p| p.comments.iter()))
    {
        if !ids.insert(c.id.clone()) {
            return Err(Error::Response("floor pages repeat ID"));
        }
    }
    let reopened = Arc::new(AccountSession::new(
        reads.clone(),
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    reopened.refresh().await?;
    let token = reopened.token()?;
    let count = reads.count.load(Ordering::SeqCst);
    let cached = Comments::new(reopened)?
        .cached_floor_for(&token, &resource, &query)?
        .ok_or(Error::State("floor cache missing on reopen"))?;
    if serde_json::to_value(&cached)? != serde_json::to_value(&first)?
        || reads.count.load(Ordering::SeqCst) != count
    {
        return Err(Error::State("floor cache mismatch or unexpected network"));
    }
    fs::write(
        root.join("pages.json"),
        serde_json::to_vec_pretty(
            &json!({"resource":resource,"query":query,"first":first,"second":second}),
        )?,
    )?;
    let report = json!({"ok":true,"first":first.comments.len(),"second":second.as_ref().map(|v|v.comments.len()),"total":first.total,"moreAfterSecond":second.as_ref().map(|v|v.more),"uniqueIds":ids.len(),"cachedReopen":true,"requests":count,"publicWrites":0,"thirdPartyParameterSource":"references/netease-api-reborn-4.29.7/package/module/comment_floor.js","protocol":"existing PC EAPI, no fallback"});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("floor read proof: {error}");
        std::process::exit(1);
    }
}
