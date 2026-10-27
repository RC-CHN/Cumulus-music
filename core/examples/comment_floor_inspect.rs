//! Explicit read-only schema observation. No comment mutations or protocol fallback.
use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    comments::{CommentResource, ResourceKind},
    protocol::{
        api::{Api, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::json;
use std::{fs, path::PathBuf, sync::Arc};
async fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 4 {
        return Err(Error::InvalidInput(
            "usage: comment_floor_inspect PC_PROFILE NEW_PRIVATE_ROOT TRACK_ID COMMENT_ID",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput("existing PC profile required"));
    }
    let resource = CommentResource {
        kind: ResourceKind::Track,
        id: identifier(&json!(args[2]))?,
        owner: None,
    };
    let parent = identifier(&json!(args[3]))?;
    fs::create_dir(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    }
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = MusicHttpClient::new(credentials, ClientProfile::default())?;
    api.account().await?;
    let payload =
        json!({"threadId":resource.thread()?,"parentCommentId":parent,"time":-1,"limit":20});
    let value = api
        .request("/api/resource/comment/floor/get", payload.clone(), false)
        .await?;
    fs::write(
        root.join("floor-0.json"),
        serde_json::to_vec_pretty(
            &json!({"path":"/api/resource/comment/floor/get","payload":payload,"response":value}),
        )?,
    )?;
    println!(
        "{}",
        json!({"code":value["code"],"keys":value.as_object().map(|v|v.keys().collect::<Vec<_>>()),"data_keys":value["data"].as_object().map(|v|v.keys().collect::<Vec<_>>()),"public_writes":0})
    );
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("comment floor observation: {error}");
        std::process::exit(1);
    }
}
