//! Read-only native relay eligibility proof. Explicit existing PC profile,
//! isolated private state, no pull, settings writes or automatic publishing.
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials},
    client::ClientError,
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    relay::{ALERT_PATH, CONFIG_PATH, RelayService, SETTING_PATH},
    storage::{StateStore, private_directory_builder},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
struct Reads {
    api: MusicHttpClient,
    calls: Mutex<Vec<Value>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/rtrs/abt/front/expinfo/list"
                    | CONFIG_PATH
                    | SETTING_PATH
                    | ALERT_PATH
            ) {
                return Err(Error::InvalidInput(
                    "relay proof permits only eligibility reads",
                ));
            }
            let response = self.api.request(path, payload, cache).await?;
            self.calls
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .push(json!({"path":path,"code":response["code"]}));
            Ok(response)
        })
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("existing PC profile required")?);
    let output = PathBuf::from(args.next().ok_or("new private proof directory required")?);
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("invalid proof arguments".into());
    }
    private_directory_builder().create(&output)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(Reads {
        api: MusicHttpClient::new(credentials, ClientProfile::default())?,
        calls: Mutex::default(),
    });
    let session = Arc::new(AccountSession::new(
        api.clone(),
        Some(Arc::new(StateStore::open(output.join("state"))?)),
    ));
    session.refresh().await?;
    let token = session.token()?;
    let service = RelayService::new(session)?;
    let result = match service.refresh_for(&token).await {
        Ok(snapshot) => {
            let cached = service.snapshot_for(&token)?;
            json!({"ok":cached.revision == snapshot.revision,"availability":snapshot.availability,"config":snapshot.config,"setting":snapshot.setting,"hasAlert":snapshot.alert.is_some(),"cacheEqual":cached.revision == snapshot.revision})
        }
        Err(error) => json!({"ok":false,"error":ClientError::from(error)}),
    };
    let report = json!({"result":result,"calls":*api.calls.lock().map_err(|_| Error::LockPoisoned)?,"writes":0,"pulls":0,"playbackVerified":false});
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if result["ok"] != true {
        return Err("relay eligibility proof failed; inspect private result".into());
    }
    Ok(())
}
