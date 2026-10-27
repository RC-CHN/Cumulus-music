//! Read-only familiar/zone discovery and queue acceptance. The credential profile is explicit;
//! business caches go into a new private directory. No feedback is submitted.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials},
    fm::{
        FmRights, FmService, FmSource,
        discovery::{FmDiscovery, FmDiscoveryQuery},
    },
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
                    | "/api/pc/daily/rcmd/block"
                    | "/api/secondary/tab/list"
                    | "/api/zone/tab/get"
                    | "/api/zone/get"
                    | "/api/tag/tab/fm/recommends"
                    | "/api/v3/song/detail"
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
    let builder = cumulus_core::storage::private_directory_builder();
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
    fm.refresh_blacklist_for(&token).await?;
    fm.refresh_trash_for(&token).await?;
    let discovery = FmDiscovery::new(session.clone())?;
    let home = discovery.page_for(&token, &FmDiscoveryQuery::Home).await?;
    fs::write(root.join("home.json"), serde_json::to_vec_pretty(&home)?)?;
    let mut sources = home
        .page
        .cards
        .iter()
        .filter(|c| matches!(c.source, FmSource::Familiar { .. }))
        .map(|c| c.source.clone())
        .collect::<Vec<_>>();
    let zone = home.page.zones.first().ok_or("no live zone entry")?;
    let query = FmDiscoveryQuery::Zone {
        page_code: zone.page_code.clone(),
        tab_key: "default".into(),
    };
    let page = discovery.page_for(&token, &query).await?;
    fs::write(root.join("zone.json"), serde_json::to_vec_pretty(&page)?)?;
    sources.extend(page.page.cards.iter().take(2).map(|c| c.source.clone()));
    let mut queues = Vec::new();
    for (i, source) in sources.iter().enumerate() {
        let queue = fm.preload_for(&token, source.clone(), rights).await?;
        fs::write(
            root.join(format!("queue-{i}.json")),
            serde_json::to_vec_pretty(&queue)?,
        )?;
        queues.push(queue);
    }
    if !sources
        .iter()
        .any(|s| matches!(s, FmSource::Familiar { .. }))
        || !sources.iter().any(|s| matches!(s, FmSource::Zone { .. }))
        || queues.iter().any(|q| q.current().is_none())
    {
        return Err("expected playable familiar and zone FM".into());
    }
    let before = reads.calls.lock().unwrap().len();
    let reopened = FmService::new(session.clone())?;
    let reopened_discovery = FmDiscovery::new(session)?;
    assert_eq!(
        reopened_discovery
            .cached_for(&token, &FmDiscoveryQuery::Home)?
            .as_ref(),
        Some(&home)
    );
    assert_eq!(
        reopened_discovery.cached_for(&token, &query)?.as_ref(),
        Some(&page)
    );
    for queue in &queues {
        assert_eq!(
            serde_json::to_value(queue)?,
            serde_json::to_value(reopened.queue_for(&token, &queue.source)?.unwrap())?
        );
    }
    assert_eq!(reads.calls.lock().unwrap().len(), before);
    let report = json!({"implementation":"rust","explicit_remote_writes":0,"home_cards":home.page.cards.len(),"zones":home.page.zones.len(),"zone_cards":page.page.cards.len(),"zone_tabs":page.page.tabs.len(),"zone_fallback":page.page.default_fallback,"queue_sizes":queues.iter().map(|q|q.tracks.len()).collect::<Vec<_>>(),"cache_reopen_equal":true,"cache_no_network":true,"calls":*reads.calls.lock().unwrap(),"full_fm_complete":false});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
