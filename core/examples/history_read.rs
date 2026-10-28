//! Existing-account recent-play reads. Private output, no history mutations.
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials},
    client::ClientError,
    history::{HistoryKind, HistoryService},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
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
    output: PathBuf,
    sequence: Mutex<usize>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/play-record/song/list"
                    | "/api/song/enhance/privilege"
                    | "/api/play-record/album/list"
                    | "/api/play-record/playlist/list"
                    | "/api/play-record/voice/list"
                    | "/api/play-record/djradio/list"
                    | "/api/play-record/voice/book/timeline/list"
            ) {
                return Err(Error::InvalidInput(
                    "history proof only permits known reads",
                ));
            }
            let response = self.api.request(path, payload.clone(), cache).await?;
            if path != "/api/w/nuser/account/get" {
                let mut sequence = self.sequence.lock().map_err(|_| Error::LockPoisoned)?;
                fs::write(
                    self.output.join(format!("response-{sequence}.json")),
                    serde_json::to_vec_pretty(
                        &json!({"path":path,"payload":payload,"response":response}),
                    )?,
                )?;
                *sequence += 1;
            }
            Ok(response)
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
        sequence: Mutex::new(0),
    });
    let store = Arc::new(StateStore::open(root.join("state"))?);
    let session = Arc::new(AccountSession::new(api, Some(store)));
    session.refresh().await?;
    let token = session.token()?;
    let service = HistoryService::new(session)?;
    let mut rows = Vec::new();
    for kind in HistoryKind::ALL {
        let result = match service.refresh_for(&token, kind).await {
            Ok(fresh) => {
                let cached = service.snapshot_for(&token, kind)?;
                let equal = serde_json::to_value(&fresh)? == serde_json::to_value(&cached)?;
                fs::write(
                    root.join(format!("{}-native.json", kind.key())),
                    serde_json::to_vec_pretty(&fresh)?,
                )?;
                json!({"scope":kind,"ok":equal,"rows":fresh.entries.len(),"remoteRows":fresh.remote.as_ref().map(|r|r.records.len()),"cacheEqual":equal})
            }
            Err(error) => json!({"scope":kind,"ok":false,"error":ClientError::from(error)}),
        };
        println!("{result}");
        rows.push(result);
    }
    let ok = rows.iter().all(|r| r["ok"] == true);
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(
            &json!({"ok":ok,"reads":rows,"explicitRemoteWrites":0,"playbackVerified":false}),
        )?,
    )?;
    if !ok {
        return Err("some recent-play reads did not pass; inspect private output".into());
    }
    Ok(())
}
