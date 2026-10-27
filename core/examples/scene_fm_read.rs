//! Read-only Scene FM discovery and complete voice queue preparation. The profile is explicit;
//! business caches go into a new private directory. No feedback is submitted.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials},
    podcast::scene::SceneFmService,
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Reads {
    api: Arc<dyn Api>,
    calls: Mutex<Vec<String>>,
    root: PathBuf,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            // A fail-closed guard also prevents future edits to this example
            // from silently sending a skip/trash/blacklist mutation.
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/voice/homepage/block/content"
                    | "/api/content/scene/fm/play/list"
                    | "/api/dj/program/detail"
            ) {
                return Err(cumulus_core::Error::State(
                    "non-read endpoint in FM read proof",
                ));
            }
            let index = {
                let mut calls = self.calls.lock().unwrap();
                calls.push(path.into());
                calls.len()
            };
            let value = self.api.request(path, payload.clone(), cache).await?;
            if !matches!(
                path,
                "/api/w/nuser/account/get" | "/api/music-vip-membership/client/vip/info"
            ) {
                fs::write(
                    self.root.join(format!("read-{index:03}.json")),
                    serde_json::to_vec_pretty(
                        &json!({"path":path,"params":payload,"response":value}),
                    )?,
                )?;
            }
            Ok(value)
        })
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("existing PC profile required")?);
    let root = PathBuf::from(args.next().ok_or("new private proof directory required")?);
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("invalid arguments".into());
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let http = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let reads = Arc::new(Reads {
        api: http,
        calls: Mutex::new(vec![]),
        root: root.clone(),
    });
    let store = Arc::new(StateStore::open(root.join("state"))?);
    let session = Arc::new(AccountSession::new(reads.clone(), Some(store)));
    session.refresh().await?;
    let token = session.token()?;
    let scene = SceneFmService::new(session.clone())?;
    let home = scene.refresh_for(&token).await?;
    fs::write(root.join("home.json"), serde_json::to_vec_pretty(&home)?)?;
    assert!(!scene.autoplay_for(&token)?);
    let (prepared, resources, index) = scene.prepare_for(&token, home.revision).await?;
    assert!(!resources.is_empty());
    fs::write(
        root.join("first-queue.json"),
        serde_json::to_vec_pretty(&json!({"state":prepared,"resources":resources,"index":index}))?,
    )?;
    let category = home
        .scenes
        .iter()
        .find(|s| s.id != home.active_id.clone().unwrap_or_default())
        .ok_or("second category missing")?;
    let selected = scene
        .select_for(&token, prepared.revision, &category.id)
        .await?;
    let (selected, more, next) = scene.prepare_for(&token, selected.revision).await?;
    assert!(!more.is_empty());
    fs::write(
        root.join("second-queue.json"),
        serde_json::to_vec_pretty(&json!({"state":selected,"resources":more,"index":next}))?,
    )?;
    let before = reads.calls.lock().unwrap().len();
    let reopened = SceneFmService::new(session)?;
    assert_eq!(
        serde_json::to_value(&selected)?,
        serde_json::to_value(reopened.cached_for(&token)?.ok_or("cache missing")?)?
    );
    assert_eq!(before, reads.calls.lock().unwrap().len());
    let report = json!({"implementation":"rust","explicit_remote_writes":0,"categories":home.scenes.len(),"queue_sizes":[resources.len(),more.len()],"cache_reopen_equal":true,"cache_no_network":true,"default_autoplay":false,"pending_preserved":selected.pending,"calls":*reads.calls.lock().unwrap(),"actual_playback_tested":false});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
