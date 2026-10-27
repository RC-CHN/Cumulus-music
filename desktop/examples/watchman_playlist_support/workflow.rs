use super::*;
pub async fn execute(root: &Path, plan: &Plan, run: &Run) -> Result<Vec<String>> {
    let mut stages = Vec::new();
    let r = run
        .command(
            root,
            "create",
            PlaylistCommand::Create {
                name: plan.name.clone(),
                private: true,
            },
        )
        .await?;
    let id = r
        .playlist_id
        .ok_or(Error::State("no confirmed fixture ID"))?;
    save(root, "playlist-fixture.json", &json!({"id":id}))?;
    stages.push("create".into());
    metadata(root, plan, run, &id, "metadata", false).await?;
    stages.push("metadata".into());
    for (label, change, ids) in [
        ("add", TrackChange::Add, plan.tracks.clone()),
        (
            "duplicate_add",
            TrackChange::Add,
            vec![plan.tracks[0].clone()],
        ),
    ] {
        let record = tracks(root, run, &id, label, change, ids).await?;
        if label == "duplicate_add" && record.phase != PlaylistPhase::Unchanged {
            return Err(Error::State("duplicate add was not a local no-op"));
        }
        stages.push(label.into());
    }
    let mut order = run.service.detail(&run.token, &id).await?.tracks;
    order.reverse();
    tracks(
        root,
        run,
        &id,
        "reorder",
        TrackChange::Reorder,
        order.clone(),
    )
    .await?;
    stages.push("reorder".into());
    for label in ["remove", "absent_remove"] {
        let record = tracks(
            root,
            run,
            &id,
            label,
            TrackChange::Delete,
            vec![order[0].clone()],
        )
        .await?;
        if label == "absent_remove" && record.phase != PlaylistPhase::Unchanged {
            return Err(Error::State("absent delete was not a local no-op"));
        }
        stages.push(label.into());
    }
    let list = run.service.list(&run.token).await?;
    let mut ids = list.group(&plan.owner, PlaylistGroup::Created);
    ids.retain(|pid| pid != &id);
    ids.push(id.clone());
    // Move only this fixture. Original created playlists retain relative order.
    if ids
        .iter()
        .filter(|pid| *pid != &id)
        .cloned()
        .collect::<Vec<_>>()
        != plan.baseline.group(&plan.owner, PlaylistGroup::Created)
    {
        return Err(Error::State("original playlist order changed externally"));
    }
    run.command(
        root,
        "order",
        PlaylistCommand::Order {
            group: PlaylistGroup::Created,
            revision: list.revision,
            ids,
        },
    )
    .await?;
    stages.push("order".into());
    tracks(root, run, &id, "clear", TrackChange::Clear, Vec::new()).await?;
    stages.push("clear".into());
    metadata(root, plan, run, &id, "clear_metadata", true).await?;
    stages.push("clear_metadata".into());
    let snapshot = run.service.detail(&run.token, &id).await?;
    if !snapshot.tracks.is_empty()
        || snapshot.summary.owner != plan.owner
        || !snapshot.summary.private
        || snapshot.summary.name != plan.renamed
    {
        return Err(Error::State("fixture cleanup snapshot changed"));
    }
    run.command(
        root,
        "delete",
        PlaylistCommand::Delete {
            id,
            revision: snapshot.revision,
        },
    )
    .await?;
    stages.push("delete".into());
    Ok(stages)
}
async fn tracks(
    root: &Path,
    run: &Run,
    id: &str,
    label: &str,
    change: TrackChange,
    ids: Vec<String>,
) -> Result<PlaylistMutation> {
    let snapshot = run.service.detail(&run.token, id).await?;
    run.command(
        root,
        label,
        PlaylistCommand::Tracks {
            id: id.into(),
            revision: snapshot.revision,
            change,
            ids,
        },
    )
    .await
}
async fn metadata(
    root: &Path,
    plan: &Plan,
    run: &Run,
    id: &str,
    label: &str,
    clear: bool,
) -> Result<PlaylistMutation> {
    let snapshot = run.service.detail(&run.token, id).await?;
    run.command(
        root,
        label,
        PlaylistCommand::Metadata {
            id: id.into(),
            revision: snapshot.revision,
            name: plan.renamed.clone(),
            tags: if clear { vec![] } else { vec!["电子".into()] },
            description: if clear {
                String::new()
            } else {
                "Rust official EAPI isolated verification".into()
            },
        },
    )
    .await
}
