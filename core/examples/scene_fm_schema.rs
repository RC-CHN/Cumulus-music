//! Explicit read-only schema probe for official scene-FM sources.
//! Output is private, because FM recommendations are account-specific.
use cumulus_core::{
    account::{AccountSession, credentials::Credentials},
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::json;
use std::{fs, io::Write, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("existing private PC profile required")?);
    let output = PathBuf::from(args.next().ok_or("new private output directory required")?);
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("invalid arguments".into());
    }
    let builder = cumulus_core::storage::private_directory_builder();
    builder.create(&output)?;
    let store = Arc::new(StateStore::open(profile)?);
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = AccountSession::new(api, Some(store));
    session.refresh().await?;
    let mut report = json!({"implementation":"rust","explicit_remote_writes":0,"checks":{}});
    for (name, path, params) in [
        (
            "scene_fm",
            "/api/voice/homepage/block/content",
            json!({"blockCode":"VOICE_HOMEPAGE_BLOCK_SCENE_FM"}),
        ),
        ("voice_history", "/api/play-record/voice/list", json!({})),
    ] {
        match session
            .run(|api, _| async move { api.call(path, params, false, &[200]).await })
            .await
        {
            Ok((_, value)) => {
                let mut options = fs::OpenOptions::new();
                options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options
                    .open(output.join(format!("{name}.json")))?
                    .write_all(&serde_json::to_vec_pretty(&value)?)?;
                report["checks"][name] = json!({"ok":true,"keys":value.as_object().map(|o|o.keys().collect::<Vec<_>>())});
            }
            Err(_) => report["checks"][name] = json!({"ok":false}),
        }
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
