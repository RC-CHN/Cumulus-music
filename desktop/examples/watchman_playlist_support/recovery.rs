use super::*;
pub async fn recover(
    original: &Path,
    credentials: Arc<Credentials>,
    profile: ClientProfile,
    context: Arc<RequestContext>,
) -> Result<Value> {
    let _lock = OperationLock::acquire(original, "probe", "playlist-workflow")?;
    let plan: Plan = serde_json::from_slice(&fs::read(original.join("playlist-plan.json"))?)?;
    if !original.join("playlist-state/core.sqlite3").is_file() {
        return Err(Error::State("original playlist journal missing"));
    }
    let output = (0..100)
        .find_map(|n| {
            let p = original.join(format!("playlist-recovery-{n}"));
            match fs::create_dir(&p) {
                Ok(()) => Some(Ok(p)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            }
        })
        .ok_or(Error::State("too many playlist recovery attempts"))??;
    let run = Run::open(original, credentials, profile, context).await?;
    if run.token.uid.as_deref() != Some(plan.owner.as_str()) {
        return Err(Error::State("playlist recovery account mismatch"));
    }
    let initial = run.backend.write_count()?;
    let mut records = run.service.operations(&run.token)?;
    for record in &mut records {
        if record.phase.pending() {
            *record = run
                .service
                .reconcile(&run.token, &record.operation_id)
                .await?;
        }
    }
    save(&output, "journal.json", &serde_json::to_value(&records)?)?;
    if records.iter().any(|r| r.phase.pending()) {
        return Err(Error::MutationUncertain);
    }
    let current = run.service.list(&run.token).await?;
    let candidates = current
        .playlists
        .iter()
        .filter(|p| {
            p.owner == plan.owner
                && p.private
                && (p.name == plan.name || p.name == plan.renamed)
                && !plan.baseline.playlists.iter().any(|old| old.id == p.id)
        })
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        return Err(Error::State("ambiguous recovery fixtures"));
    }
    if let Some(created) = candidates.first() {
        if !records.iter().any(|r| {
            matches!(&r.command,PlaylistCommand::Create{name,private:true}if name==&plan.name)
                && r.phase == PlaylistPhase::Confirmed
                && r.playlist_id.as_ref() == Some(&created.id)
        }) {
            return Err(Error::State(
                "fixture does not match confirmed original create",
            ));
        }
        let p = run.service.detail(&run.token, &created.id).await?;
        if p.summary.owner != plan.owner
            || !p.summary.private
            || p.summary.favorite
            || (p.summary.name != plan.name && p.summary.name != plan.renamed)
            || p.tracks.iter().any(|id| !plan.tracks.contains(id))
            || (p.description != "Rust official EAPI isolated verification"
                && !p.description.is_empty())
            || (!p.tags.is_empty() && p.tags != ["电子"])
        {
            return Err(Error::State(
                "fixture content changed outside planned workflow",
            ));
        }
        save(&output, "owned.json", &serde_json::to_value(&p)?)?;
        // Explicit cleanup intent after all prior domain operations have a
        // terminal outcome. Unknown delete is never retried with another wire.
        if !original.join("sent-cleanup.json").exists() {
            run.backend.permit(Permit::cleanup(&created.id))?;
            let result = PlaylistApi::new(run.cleanup.clone())
                .delete(&json!(created.id))
                .await;
            run.backend.disarm()?;
            save(
                &output,
                "cleanup.json",
                &match result {
                    Ok(v) => json!({"acknowledged":true,"reply":v}),
                    Err(e) => json!({"acknowledged":false,"error":e.to_string()}),
                },
            )?;
        }
    }
    let mut restored = false;
    for i in 0..4 {
        let list = run.service.list(&run.token).await?;
        save(
            &output,
            &format!("list-{i}.json"),
            &serde_json::to_value(&list)?,
        )?;
        if serde_json::to_value(list.playlists)? == serde_json::to_value(&plan.baseline.playlists)?
        {
            restored = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let report = json!({"ok":restored,"recoveredOriginalJournal":true,"baselineRestored":restored,"businessWrites":run.backend.write_count()?-initial,"cleanupRequired":!restored,"workflowPassed":false});
    save(&output, "result.json", &report)?;
    if !restored {
        return Err(Error::MutationUncertain);
    }
    Ok(report)
}
