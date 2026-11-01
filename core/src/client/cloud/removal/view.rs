use super::*;
pub(super) fn project(
    context: &Context,
    plan: &Plan,
    remote: Option<DeleteOperation>,
    running: bool,
) -> Result<CloudRemovalView> {
    context.guard()?;
    let retry_local = plan.choice == Some(true)
        && plan.files.iter().any(|f| {
            matches!(
                f.state,
                CloudRemovalFileState::Failed | CloudRemovalFileState::Removing
            )
        });
    let can_continue = plan.choice.is_some()
        && !running
        && !plan.blocked
        && remote.as_ref().is_none_or(|r| {
            r.phase == DeletePhase::Preparing || (!r.phase.pending() && retry_local)
        });
    let phase = if plan.blocked {
        CloudRemovalPhase::Result
    } else if let Some(ref r) = remote {
        if r.phase == DeletePhase::Preparing {
            CloudRemovalPhase::Local
        } else if r.phase.pending() {
            CloudRemovalPhase::Remote
        } else {
            CloudRemovalPhase::Result
        }
    } else if plan.choice.is_some() {
        CloudRemovalPhase::Local
    } else {
        CloudRemovalPhase::Prepared
    };
    Ok(CloudRemovalView {
        epoch: context.token.generation.to_string(),
        id: plan.id.clone(),
        revision: plan.revision.clone(),
        ids: plan.ids.clone(),
        titles: plan.titles.clone(),
        kinds: plan.kinds.clone(),
        phase,
        running,
        delete_local: plan.choice,
        local_count: plan.files.len(),
        cue_count: plan.cue_count,
        local_issue: plan
            .local_issue
            .then_some("部分本地关联无法核对，未纳入本地删除范围"),
        files: plan
            .files
            .iter()
            .map(|f| CloudRemovalFileView {
                cloud_id: f.cloud_id.clone(),
                title: f.title.clone(),
                state: f.state,
                issue: f.issue.map(Issue::message),
            })
            .collect(),
        deletion: remote.map(Into::into),
        legacy_only: false,
        can_continue,
        error: plan.issue.map(Issue::message),
    })
}
pub(super) fn legacy(
    context: &Context,
    op: DeleteOperation,
    running: bool,
) -> Result<CloudRemovalView> {
    context.guard()?;
    let snapshot = context.library.cached()?;
    let titles = op
        .ids
        .iter()
        .map(|id| {
            snapshot
                .as_ref()
                .and_then(|s| s.items.iter().find(|r| &r.song_id == id))
                .and_then(|r| r.song["name"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("云盘歌曲 {id}"))
        })
        .collect();
    Ok(CloudRemovalView {
        epoch: context.token.generation.to_string(),
        id: op.id.clone(),
        revision: op.before_revision.clone(),
        ids: op.ids.clone(),
        titles,
        kinds: vec![crate::cloud::library::CloudDeletionKind::Unknown; op.ids.len()],
        phase: if op.phase.pending() {
            CloudRemovalPhase::Remote
        } else {
            CloudRemovalPhase::Result
        },
        running,
        delete_local: Some(false),
        local_count: 0,
        cue_count: 0,
        local_issue: None,
        files: vec![],
        deletion: Some(op.into()),
        legacy_only: true,
        can_continue: false,
        error: None,
    })
}
