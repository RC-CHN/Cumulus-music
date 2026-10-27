//! Authorized raw-media integration probe. Explicitly fetches download URLs,
//! pauses/reopens/renews, verifies complete bytes, and never writes account data.
use cumulus_core::{
    Error, Result,
    account::{AccountApi, AccountSession, SessionToken, credentials::Credentials},
    catalog::CatalogApi,
    download::transfer::{
        CreateTransfer, MediaBackend, MediaReply, MediaRequest, MediaUrl, ReqwestMediaBackend,
        RunOptions, TransferDirective, TransferManager, TransferResource, TransferStatus,
    },
    protocol::{
        api::{ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Observed {
    inner: ReqwestMediaBackend,
    requests: Mutex<Vec<Value>>,
}
impl MediaBackend for Observed {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let offset = request.offset;
            let conditional = request.if_range.is_some();
            let reply = self.inner.get(request).await?;
            self.requests.lock().map_err(|_| Error::LockPoisoned)?.push(
                json!({"offset":offset,"conditional":conditional,"http_status":reply.status}),
            );
            Ok(reply)
        })
    }
}
async fn authorize(
    session: &AccountSession,
    token: &SessionToken,
    id: &str,
) -> Result<(TransferResource, Value)> {
    let row = session
        .run_for(token, |api, _| async move {
            CatalogApi::new(api)
                .download_url(&json!(id), "lossless")
                .await
        })
        .await?
        .ok_or(Error::Response("missing download resource"))?;
    if row["code"] != 200 || !row["freeTrialInfo"].is_null() {
        return Err(Error::Response("download not authorized as full media"));
    }
    if let Some(actual) = row.get("id").filter(|v| !v.is_null())
        && identifier(actual)? != id
    {
        return Err(Error::Response("download returned another song"));
    }
    let resource = TransferResource {
        url: MediaUrl::parse(
            row["url"]
                .as_str()
                .ok_or(Error::Response("missing download URL"))?,
        )?,
        size: row["size"].as_u64(),
        md5: row["md5"].as_str().map(str::to_owned),
    };
    if resource.md5.is_none() || resource.size.is_none() {
        return Err(Error::Response("probe requires server size and MD5"));
    }
    Ok((
        resource,
        json!({"level":row["level"],"format":row["type"],"size":row["size"]}),
    ))
}
async fn run() -> Result<Value> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: transfer_read EXISTING_PC_PROFILE NEW_OUTPUT_DIRECTORY",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing profile required"));
    }
    let output = PathBuf::from(&args[1]);
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&output)?;
    let store = Arc::new(StateStore::open(profile)?);
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(api, Some(store.clone())));
    session.refresh().await?;
    let token = session.token()?;
    let owner = token.uid.clone().ok_or(Error::State("not authenticated"))?;
    let ids = session
        .run_for(&token, |api, uid| async move {
            AccountApi::new(api).liked_ids(&json!(uid)).await
        })
        .await?;
    let mut report = json!({"schema":1,"implementation":"rust-core","remote_mutations":0,"stage":"raw_media_transfer","cases":[]});
    for (index, id) in ids.into_iter().take(2).enumerate() {
        let (resource, mut actual) = authorize(&session, &token, &id).await?;
        let backend = Arc::new(Observed {
            inner: ReqwestMediaBackend::new(Duration::from_secs(30))?,
            requests: Mutex::new(Vec::new()),
        });
        let manager = TransferManager::with_backend(
            store.clone(),
            owner.clone(),
            backend.clone(),
            512 * 1024 * 1024,
        )?;
        let job = manager.create(CreateTransfer {
            resource,
            target: output.join(format!("track-{index}.media")),
            resource_id: Some(id.clone()),
            id: None,
        })?;
        let control = {
            let session = session.clone();
            let token = token.clone();
            Arc::new(move || {
                if session.token()? == token {
                    Ok(TransferDirective::Continue)
                } else {
                    Err(Error::StaleResponse)
                }
            })
        };
        let partial = manager
            .run(
                &job.id,
                RunOptions {
                    byte_budget: Some(65537),
                    control: control.clone(),
                },
            )
            .await?;
        let paused = partial.status == TransferStatus::Paused;
        let mut authorizations = 1;
        let reopened = TransferManager::with_backend(
            Arc::new(StateStore::open(store.directory())?),
            owner.clone(),
            backend.clone(),
            512 * 1024 * 1024,
        )?;
        let final_job = if paused {
            let (renewed, info) = authorize(&session, &token, &id).await?;
            actual = info;
            authorizations += 1;
            reopened.refresh_resource(&job.id, renewed)?;
            reopened
                .run(
                    &job.id,
                    RunOptions {
                        byte_budget: None,
                        control,
                    },
                )
                .await?
        } else {
            partial.clone()
        };
        if final_job.status != TransferStatus::Complete
            || final_job.cleanup_pending
            || job.part.exists()
        {
            return Err(Error::State("raw transfer did not finish cleanly"));
        }
        report["cases"].as_array_mut().expect("array").push(json!({"case":index,"ok":true,"paused":paused,"pause_bytes":partial.bytes,"authorizations":authorizations,"actual_resource":actual,"integrity":final_job.integrity,"requests":backend.requests.lock().map_err(|_|Error::LockPoisoned)?.clone(),"part_cleaned":true}));
    }
    if report["cases"].as_array().is_none_or(|c| c.len() != 2) {
        return Err(Error::State("probe requires two source songs"));
    }
    Ok(report)
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).expect("report")),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
