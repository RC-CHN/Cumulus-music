//! Explicit current-account comment proof. New private directory; no publishing.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials, resources::MutationPhase},
    comments::*,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{fs, io::Write, path::Path, sync::Arc, time::Duration};
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut f = fs::File::create(root.join(name))?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
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
    if intent["owner"].as_str() != token.uid.as_deref() || intent["before_liked"] != false {
        return Err(Error::State("comment cleanup owner or baseline mismatch"));
    }
    let resource: CommentResource = serde_json::from_value(intent["resource"].clone())?;
    let id = intent["comment_id"]
        .as_str()
        .ok_or(Error::State("comment cleanup identity missing"))?;
    let service = Comments::new(session.clone())?;
    let key = format!("{}:{id}", resource.thread()?);
    let journal: CommentLike = serde_json::from_value(
        StateStore::open(root.join("state"))?
            .get(
                token.uid.as_deref().expect("owner"),
                "comment-likes-v1",
                &key,
            )?
            .ok_or(Error::State("comment write intent is not established"))?,
    )?;
    if journal.desired && journal.before {
        return Err(Error::State(
            "comment was already liked at final preflight; preserve it",
        ));
    }
    let current = service.target_for(&token, &resource, id).await?;
    if current.liked == Some(true) {
        if journal.phase.pending() {
            service.reconcile_for(&token, &resource, id).await?;
        }
        service.like_for(&token, &resource, id, false).await?;
    } else if current.liked != Some(false)
        || journal.desired && journal.phase.pending() && !journal.acknowledged
    {
        return Err(Error::MutationUncertain);
    }
    for _ in 0..2 {
        if service.target_for(&token, &resource, id).await?.liked != Some(false) {
            return Err(Error::MutationUncertain);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    save(
        root,
        "restored.json",
        &json!({"original_flag_preserved":true,"liked":false,"independent_reads":2}),
    )
}
async fn run(profile: &Path, input: &Path, root: &Path, mutate: bool) -> Result<()> {
    fs::create_dir(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    let resources: Vec<CommentResource> = serde_json::from_slice(&fs::read(input)?)?;
    let session = account(profile, root).await?;
    let token = session.token()?;
    let service = Comments::new(session.clone())?;
    let mut report = vec![];
    let mut candidate = None;
    for (index, resource) in resources.iter().enumerate() {
        let q = CommentQuery {
            limit: 20,
            ..Default::default()
        };
        let first = service.page_for(&token, resource, &q).await?;
        save(
            root,
            &format!("resource-{index}-latest.json"),
            &serde_json::to_value(&first)?,
        )?;
        let hot = service
            .page_for(
                &token,
                resource,
                &CommentQuery {
                    sort: CommentSort::Hot,
                    ..q.clone()
                },
            )
            .await?;
        save(
            root,
            &format!("resource-{index}-hot.json"),
            &serde_json::to_value(&hot)?,
        )?;
        let mut pages = 0;
        for (name, page) in [("latest", &first), ("hot", &hot)] {
            if let Some(next) = &page.next {
                let second = service.page_for(&token, resource, next).await?;
                let before = page
                    .comments
                    .iter()
                    .map(|c| c.id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                if second.comments.is_empty()
                    || second
                        .comments
                        .iter()
                        .any(|c| before.contains(c.id.as_str()))
                {
                    return Err(Error::Response(
                        "comment proof pagination did not advance distinctly",
                    ));
                }
                save(
                    root,
                    &format!("resource-{index}-{name}-next.json"),
                    &serde_json::to_value(second)?,
                )?;
                pages += 1;
            }
        }
        let mut located = false;
        if let Some(item) = first.comments.first().or(hot.comments.first()) {
            let target = service.target_for(&token, resource, &item.id).await?;
            located = target.id == item.id;
        }
        if matches!(resource.kind, ResourceKind::Track | ResourceKind::Voice) {
            let info = session
                .run_for(&token, |api, _| async move {
                    CommentApi::new(api)
                        .information(resource.kind, std::slice::from_ref(&resource.id))
                        .await
                })
                .await?;
            save(root, &format!("resource-{index}-information.json"), &info)?;
        }
        if candidate.is_none() && resource.kind == ResourceKind::Track {
            candidate = first
                .comments
                .iter()
                .chain(&hot.comments)
                .find(|c| c.liked == Some(false))
                .map(|c| (resource.clone(), c.id.clone()));
        }
        report.push(json!({"kind":resource.kind,"latest_count":first.comments.len(),"latest_total":first.total,"hot_count":hot.comments.len(),"hot_total":hot.total,"next_pages_checked":pages,"target_located":located}));
    }
    if mutate {
        let (resource, id) =
            candidate.ok_or(Error::State("no explicitly unliked comment candidate"))?;
        let before = service.target_for(&token, &resource, &id).await?;
        if before.liked != Some(false) {
            return Err(Error::State("preserve already liked comment"));
        }
        save(
            root,
            "restore-intent.json",
            &json!({"owner":token.uid,"resource":resource,"comment_id":id,"before_liked":false}),
        )?;
        let outcome = service.like_for(&token, &resource, &id, true).await;
        let cleanup = restore(session.clone(), root).await;
        let result = outcome?;
        if result.phase != MutationPhase::Confirmed || result.before {
            return Err(Error::State(
                "comment mutation was not isolated and confirmed",
            ));
        }
        cleanup?;
    }
    save(
        root,
        "report.json",
        &json!({"ok":true,"resources":report,"like_roundtrip":mutate,"comments_published":0,"messages_sent":0,"restored":!mutate||root.join("restored.json").exists()}),
    )?;
    println!(
        "comment proof complete; resources={}, isolated_like={mutate}",
        resources.len()
    );
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 3 && args[1] == "--restore" {
        let root = Path::new(&args[2]);
        restore(account(Path::new(&args[0]), root).await?, root).await?;
        println!("comment baseline restored");
        return Ok(());
    }
    if !(3..=4).contains(&args.len()) || args.get(3).is_some_and(|v| v != "--like") {
        return Err(Error::InvalidInput(
            "PROFILE RESOURCE_JSON NEW_PROOF [--like], or PROFILE --restore PROOF",
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
