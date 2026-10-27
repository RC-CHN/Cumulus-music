//! Explicit authorized-account proof. No publishing, messages, or payments.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    mv::MvService,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, sync::Arc, time::Duration};
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut file = fs::File::create(root.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
async fn account(profile: &Path, root: &Path) -> Result<Arc<AccountSession>> {
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(
        api,
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    session.refresh().await?;
    Ok(session)
}
async fn restore(session: Arc<AccountSession>, root: &Path) -> Result<()> {
    let intent: Value = serde_json::from_slice(&fs::read(root.join("restore-intent.json"))?)?;
    let token = session.token()?;
    if intent["owner"].as_str() != token.uid.as_deref() || intent["before"] != false {
        return Err(Error::State("MV cleanup owner or baseline mismatch"));
    }
    let id = intent["id"]
        .as_str()
        .ok_or(Error::State("MV cleanup identity missing"))?;
    let service = MvService::new(session)?;
    let journal = service
        .like_operation_for(&token, id)?
        .ok_or(Error::State("MV durable write intent missing"))?;
    if journal.before && journal.desired {
        return Err(Error::State("MV liked before final preflight; preserve it"));
    }
    let current = service.like_state_for(&token, id).await?;
    if current.liked {
        // A previous completed restoration cannot authorize undoing a new external like.
        if root.join("restored.json").exists() {
            return Err(Error::State("MV changed after completed restoration"));
        }
        if journal.phase.pending() {
            service.reconcile_like_for(&token, id).await?;
        }
        service.like_for(&token, id, false).await?;
    } else if journal.desired && journal.phase.pending() && !journal.acknowledged {
        return Err(Error::MutationUncertain);
    }
    for _ in 0..2 {
        if service.like_state_for(&token, id).await?.liked {
            return Err(Error::MutationUncertain);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    save(
        root,
        "restored.json",
        &json!({"baseline_preserved":true,"liked":false,"independent_reads":2}),
    )
}
async fn run(profile: &Path, input: &Path, root: &Path, mutate: bool) -> Result<()> {
    fs::create_dir(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    let input: Value = serde_json::from_slice(&fs::read(input)?)?;
    let id = input["id"]
        .as_str()
        .ok_or(Error::InvalidInput("explicit MV identifier required"))?;
    let session = account(profile, root).await?;
    let token = session.token()?;
    let service = MvService::new(session.clone())?;
    let detail = service.download_options_for(&token, &json!(id)).await?;
    save(root, "detail.json", &serde_json::to_value(&detail)?)?;
    let recommendations = service.recommendations_for(&token, id).await?;
    save(
        root,
        "recommendations.json",
        &serde_json::to_value(&recommendations)?,
    )?;
    let state = service.like_state_for(&token, id).await?;
    if mutate {
        if state.liked {
            return Err(Error::State("preserve already liked MV"));
        }
        save(
            root,
            "restore-intent.json",
            &json!({"owner":token.uid,"id":id,"before":false}),
        )?;
        let operation = service.like_for(&token, id, true).await;
        let cleanup = restore(session, root).await;
        operation?;
        cleanup?;
    }
    save(
        root,
        "report.json",
        &json!({"ok":true,"like_state_known":true,"related_songs":detail.related_songs.len(),
        "related_privilege_failed":detail.related_privilege_failed,"related_privileges_present":detail.related_songs.iter().filter(|v|!v["privilege"].is_null()).count(),
        "recommendations":recommendations.len(),"like_roundtrip":mutate,"baseline_preserved":!mutate||root.join("restored.json").exists(),"publications":0,"messages":0,"payments":0}),
    )
}
async fn execute() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 3 && args[1] == "--restore" {
        let root = Path::new(&args[2]);
        return restore(account(Path::new(&args[0]), root).await?, root).await;
    }
    if !(args.len() == 3 || args.len() == 4 && args[3] == "--like") {
        return Err(Error::InvalidInput(
            "PROFILE PRIVATE_INPUT NEW_PROOF [--like], or PROFILE --restore EXISTING_PROOF",
        ));
    }
    run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        args.len() == 4,
    )
    .await
}
#[tokio::main]
async fn main() {
    if let Err(error) = execute().await {
        let public = cumulus_core::client::ClientError::from(error);
        eprintln!("MV proof failed: {}", public.code);
        std::process::exit(1);
    }
}
