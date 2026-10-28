//! Explicit existing-account, read-only collection evidence; never run in CI.
use cumulus_core::{
    Error,
    account::{
        AccountSession,
        collections::{CollectionQuery, Collections},
        credentials::Credentials,
        profiles::ProfileRegistry,
    },
    client::{ClientError, MusicClient},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::{StateStore, private_directory_builder},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Reads {
    api: MusicHttpClient,
    output: PathBuf,
    count: Mutex<usize>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/album/sublist"
                    | "/api/digitalAlbum/purchased"
                    | "/api/user/follow/users/mixed/get/v2"
            ) {
                return Err(Error::InvalidInput(
                    "collection proof permits known reads only",
                ));
            }
            let value = self.api.request(path, payload.clone(), cache).await?;
            if path != "/api/w/nuser/account/get" {
                let mut count = self.count.lock().map_err(|_| Error::LockPoisoned)?;
                fs::write(
                    self.output.join(format!("response-{count}.json")),
                    serde_json::to_vec_pretty(
                        &json!({"path":path,"payload":payload,"response":value}),
                    )?,
                )?;
                *count += 1;
            }
            Ok(value)
        })
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("existing PC profile required")?);
    let root = PathBuf::from(args.next().ok_or("new private output directory required")?);
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("unexpected arguments or missing PC profile".into());
    }
    private_directory_builder().create(&root)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(Reads {
        api: MusicHttpClient::new(credentials, ClientProfile::default())?,
        output: root.clone(),
        count: Mutex::new(0),
    });
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry);
    if let Err(error) = session.refresh().await {
        println!(
            "{}",
            json!({"ok":false,"stage":"account","error":ClientError::from(error)})
        );
        return Err("account restore failed".into());
    }
    let token = session.token()?;
    let service = Collections::new(session.clone())?;
    let mut results = Vec::new();
    for (scope, first) in [
        (
            "saved",
            CollectionQuery::SavedAlbums {
                offset: 0,
                limit: 50,
            },
        ),
        ("purchased", CollectionQuery::PurchasedAlbums),
        (
            "followed",
            CollectionQuery::FollowedArtists {
                cursor: "0".into(),
                size: 19,
            },
        ),
    ] {
        let mut query = first;
        let mut seen = HashSet::new();
        for page in 0..500 {
            if !seen.insert(serde_json::to_string(&query)?) {
                return Err("repeated collection cursor".into());
            }
            match client
                .collection_page(&token.generation.to_string(), query.clone(), true)
                .await
            {
                Ok(value) => {
                    let view = value.value.ok_or("missing collection view")?;
                    let cached = service
                        .cached_for(&token, &query)?
                        .ok_or("missing native snapshot")?;
                    let replay = client
                        .collection_page(&token.generation.to_string(), query.clone(), false)
                        .await?
                        .value
                        .ok_or("missing cache DTO")?;
                    assert_eq!(
                        serde_json::to_value(&view.page)?,
                        serde_json::to_value(replay.page)?
                    );
                    fs::write(
                        root.join(format!("native-{scope}-{page}.json")),
                        serde_json::to_vec_pretty(&cached)?,
                    )?;
                    fs::write(
                        root.join(format!("view-{scope}-{page}.json")),
                        serde_json::to_vec_pretty(&view)?,
                    )?;
                    let result = json!({"ok":true,"scope":scope,"page":page,"records":cached.data.records.len(),"total":view.total,"more":view.next.is_some(),"cacheEqual":true});
                    println!("{result}");
                    results.push(result);
                    if let Some(next) = view.next {
                        if page == 499 {
                            return Err("collection page limit exceeded".into());
                        }
                        query = next;
                    } else {
                        break;
                    }
                }
                Err(error) => {
                    let result = json!({"ok":false,"scope":scope,"page":page,"error":ClientError::from(error)});
                    println!("{result}");
                    results.push(result);
                    break;
                }
            }
        }
    }
    let ok = results.iter().all(|r| r["ok"] == true);
    fs::write(
        root.join("summary.json"),
        serde_json::to_vec_pretty(&json!({"ok":ok,"pages":results,"explicitWrites":0}))?,
    )?;
    if !ok {
        return Err("collection read failed; inspect private evidence".into());
    }
    Ok(())
}
