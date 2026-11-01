//! Explicit read-only proof: the guard cannot dispatch correction submissions.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    lyrics::reports::{Reports, TYPES_PATH, Target},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
struct Reads {
    api: Arc<dyn Api>,
    calls: Mutex<Vec<String>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path != "/api/w/nuser/account/get" && path != TYPES_PATH {
                return Err(Error::State("non-read endpoint in lyric report proof"));
            }
            self.calls.lock().unwrap().push(path.into());
            self.api.request(path, payload, cache).await
        })
    }
}
async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: lyric_report_read <private-profile> <new-proof-directory>",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing private profile required"));
    }
    cumulus_core::storage::private_directory_builder().create(&root)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let reads = Arc::new(Reads {
        api: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        calls: Mutex::new(Vec::new()),
    });
    let session = Arc::new(AccountSession::new(
        reads.clone(),
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    let result = async {
        session.refresh().await?;
        let reports = Reports::new(session.clone(), session.token()?)?;
        let context = reports.open(Target {
            song_id: "1".into(),
            title: "类型列表只读验证".into(),
            lyric_version: None,
        })?;
        let value = reports.types(&context.id, &context.revision).await?;
        reports.abandon(&value.id, &value.revision)?;
        Ok::<_, Error>(value.options)
    }
    .await;
    let report = match result {
        Ok(options) => {
            json!({"ok":true,"options":options,"calls":reads.calls.lock().unwrap().clone(),"feedbackSubmissions":0,"accountResourcesModified":false})
        }
        Err(error) => {
            // Type labels are public contract data. This second, read-only
            // observation cannot establish an authenticated-account success.
            let public = match reads
                .call(TYPES_PATH, json!({}), false, &[200])
                .await
                .and_then(|v| cumulus_core::lyrics::reports::parse_types(&v))
            {
                Ok(options) => json!({"ok":true,"options":options}),
                Err(error) => {
                    json!({"ok":false,"error":cumulus_core::client::ClientError::from(error)})
                }
            };
            json!({"ok":false,"error":cumulus_core::client::ClientError::from(error),"publicTypes":public,"authenticatedAccountVerified":false,"calls":reads.calls.lock().unwrap().clone(),"feedbackSubmissions":0,"accountResourcesModified":false})
        }
    };
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!("{report}");
            if report["ok"] != true {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("{}", cumulus_core::client::ClientError::from(error).message);
            std::process::exit(1);
        }
    }
}
