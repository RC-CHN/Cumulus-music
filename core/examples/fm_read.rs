//! Read-only live FM domain acceptance. The credential profile is explicit;
//! business caches go into a new private directory. No feedback is submitted.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials},
    fm::{FmRights, FmService, FmSource},
    membership::MembershipManager,
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
                    | "/api/music-vip-membership/client/vip/info"
                    | "/api/v1/radio/get"
                    | "/api/v3/radio/trash/get"
                    | "/api/music/dislike/get"
                    | "/api/personalized/dislike/reason"
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
    let rights = FmRights::from(&MembershipManager::new(session.clone())?.refresh().await?);
    let fm = FmService::new(session.clone())?;
    let blacklist = fm.refresh_blacklist_for(&token).await?;
    let trash = fm.refresh_trash_for(&token).await?;
    let queue = fm.preload_for(&token, FmSource::Personal, rights).await?;
    fs::write(root.join("queue.json"), serde_json::to_vec_pretty(&queue)?)?;
    fs::write(root.join("trash.json"), serde_json::to_vec_pretty(&trash)?)?;
    fs::write(
        root.join("blacklist.json"),
        serde_json::to_vec_pretty(&blacklist)?,
    )?;
    let current = queue.current().ok_or("no playable FM sample")?;
    let reasons = fm.reasons_for(&token, &current.id).await?;
    fs::write(
        root.join("reasons.json"),
        serde_json::to_vec_pretty(&reasons)?,
    )?;
    let before = reads.calls.lock().unwrap().len();
    let reopened = FmService::new(session)?;
    let cached = reopened
        .preload_for(&token, FmSource::Personal, rights)
        .await?;
    assert_eq!(before, reads.calls.lock().unwrap().len());
    assert_eq!(
        serde_json::to_value(&queue)?,
        serde_json::to_value(&cached)?
    );
    let report = json!({"implementation":"rust","explicit_remote_writes":0,"queue_tracks":queue.tracks.len(),"current_present":true,
        "algorithms_present":queue.tracks.iter().filter(|t|!t.algorithm.is_empty()).count(),"trash_tracks":trash.total,
        "blacklist_songs":blacklist.songs.len(),"blacklist_artists":blacklist.artists.len(),
        "reason_entries":reasons["data"].as_array().map(Vec::len),"cache_reopen_equal":true,"cache_no_network":true,
        "calls":*reads.calls.lock().unwrap(),"full_fm_complete":false});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
