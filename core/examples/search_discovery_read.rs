//! Explicit account read proof; never run by public CI. No history or resource writes.
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    catalog::search::{SearchDiscovery, SearchDiscoveryQuery as Q},
    client::{ClientError, MusicClient},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::{StateStore, private_directory_builder},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
struct Reads(Arc<dyn Api>);
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if ![
                "/api/w/nuser/account/get",
                "/api/search/default/keyword/get",
                "/api/search/pc/rcmd/keyword/get",
                "/api/search/pc/chart/list",
                "/api/search/pc/chart/detail",
                "/api/search/pc/suggest/keyword/get",
            ]
            .contains(&path)
            {
                return Err(Error::InvalidInput(
                    "search evidence only allows official reads",
                ));
            }
            self.0.request(path, args, cache).await
        })
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("explicit existing PC profile required")?);
    let root = PathBuf::from(args.next().ok_or("new private output directory required")?);
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("invalid arguments".into());
    }
    private_directory_builder().create(&root)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(Reads(Arc::new(MusicHttpClient::new(
        credentials,
        ClientProfile::default(),
    )?)));
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
    let session = Arc::new(AccountSession::new(api, Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry);
    if let Err(error) = session.refresh().await {
        let report = json!({"ok":false,"stage":"account","error":ClientError::from(error),"resourceWrites":0});
        fs::write(
            root.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{report}");
        return Err("account read failed".into());
    }
    let epoch = client.account()?.epoch;
    let mut checks = Vec::new();
    let mut chart = None;
    for (name, query) in [
        ("defaults", Q::Defaults),
        (
            "recommendations",
            Q::Recommendations {
                last_query: String::new(),
            },
        ),
        ("charts", Q::Charts),
        (
            "suggestions",
            Q::Suggestions {
                keyword: "夜曲".into(),
            },
        ),
    ] {
        match client.search_discovery(&epoch, query).await {
            Ok(result) => {
                let count = match &result.value {
                    SearchDiscovery::Defaults { words, .. }
                    | SearchDiscovery::Recommendations { words } => words.len(),
                    SearchDiscovery::Charts { charts } => {
                        chart = charts.first().map(|c| c.id.clone());
                        charts.len()
                    }
                    SearchDiscovery::Suggestions {
                        recommended,
                        suggestions,
                        ..
                    } => recommended.len() + suggestions.len(),
                    _ => 0,
                };
                fs::write(
                    root.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(&result)?,
                )?;
                checks.push(json!({"name":name,"ok":true,"count":count}));
            }
            Err(error) => {
                checks.push(json!({"name":name,"ok":false,"error":ClientError::from(error)}))
            }
        }
    }
    if let Some(id) = chart {
        match client.search_discovery(&epoch, Q::Chart { id }).await {
            Ok(result) => {
                fs::write(root.join("chart.json"), serde_json::to_vec_pretty(&result)?)?;
                checks.push(json!({"name":"chart","ok":true}));
            }
            Err(error) => {
                checks.push(json!({"name":"chart","ok":false,"error":ClientError::from(error)}))
            }
        }
    }
    let report = json!({"ok":checks.iter().all(|c|c["ok"]==true),"checks":checks,"implementation":"rust-client","resourceWrites":0,"historyWrites":0});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
