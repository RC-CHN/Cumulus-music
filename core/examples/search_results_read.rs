//! Explicit PC account read proof; raw private evidence never belongs in CI.
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    catalog::search::results::{self, SearchCategory, SearchResultQuery},
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
            if path != "/api/w/nuser/account/get" && !results::READ_PATHS.contains(&path) {
                return Err(Error::InvalidInput(
                    "search proof only allows official reads",
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
    let root = PathBuf::from(args.next().ok_or("new private output root required")?);
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
    let registry = ProfileRegistry::open(&root.join("projection-state"), ApiOrigin::Pc)?;
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session.refresh().await?;
    let client = MusicClient::with_session(session, registry);
    let epoch = client.account()?.epoch;
    let mut checks = Vec::new();
    let keyword = "周杰伦";
    let tabs = results::tabs(api.as_ref(), keyword).await;
    save(&root, "tabs", tabs, &mut checks)?;
    for category in SearchCategory::ALL {
        let name = serde_json::to_value(category)?.as_str().unwrap().to_owned();
        let q = SearchResultQuery {
            keyword: keyword.into(),
            category,
            offset: 0,
            limit: 6,
            correct: true,
        };
        let reply = client
            .search_results(&epoch, q)
            .await
            .and_then(|v| Ok(serde_json::to_value(v)?));
        save(&root, &name, reply, &mut checks)?;
    }
    let report = json!({"checks":checks,"resourceWrites":0,"implementation":"rust-core-typed-client","privateEvidence":true});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
fn save(
    root: &std::path::Path,
    name: &str,
    result: cumulus_core::Result<Value>,
    checks: &mut Vec<Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    match result {
        Ok(value) => {
            fs::write(
                root.join(format!("{name}.json")),
                serde_json::to_vec_pretty(&value)?,
            )?;
            checks.push(json!({"name":name,"ok":true}));
        }
        Err(e) => checks.push(json!({"name":name,"ok":false,"error":ClientError::from(e)})),
    };
    Ok(())
}
