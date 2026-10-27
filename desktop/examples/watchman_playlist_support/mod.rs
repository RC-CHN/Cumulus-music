//! Explicit isolated official-EAPI playlist workflow, using the real domain
//! journal and request context. No frontend-selected resource or mutation.
mod guard;
mod recovery;
mod workflow;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken, credentials::Credentials},
    playlist::{
        PlaylistApi, PlaylistCommand, PlaylistGroup, PlaylistList, PlaylistMutation, PlaylistPhase,
        PlaylistService, TrackChange,
    },
    protocol::{context::RequestContext, http::*},
    storage::{OperationLock, StateStore},
    watchman::{EntropyClock, SystemEntropyClock},
};
use guard::{Backend, Permit};
pub use recovery::recover;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Plan {
    owner: String,
    name: String,
    renamed: String,
    baseline: PlaylistList,
    tracks: Vec<String>,
}
struct Run {
    service: PlaylistService,
    token: SessionToken,
    backend: Arc<Backend>,
    cleanup: Arc<MusicHttpClient>,
}
impl Run {
    async fn open(
        root: &Path,
        credentials: Arc<Credentials>,
        profile: ClientProfile,
        context: Arc<RequestContext>,
    ) -> Result<Self> {
        let backend = Arc::new(Backend::new(root.to_owned())?);
        let api = Arc::new(MusicHttpClient::with_backend(
            credentials.clone(),
            profile.clone(),
            RequestOptions {
                routing: RoutingPolicy::OfficialPcPlaylists,
                ..Default::default()
            },
            Some(context.clone()),
            backend.clone(),
        )?);
        let session = Arc::new(AccountSession::new(
            api,
            Some(Arc::new(StateStore::open(root.join("playlist-state"))?)),
        ));
        session.refresh().await?;
        let token = session.token()?;
        *backend.owner.lock().map_err(|_| Error::LockPoisoned)? = token
            .uid
            .clone()
            .ok_or(Error::State("playlist proof requires account"))?;
        let cleanup = Arc::new(MusicHttpClient::with_backend(
            credentials,
            profile,
            RequestOptions::default(),
            Some(context),
            backend.clone(),
        )?);
        Ok(Self {
            service: PlaylistService::new(session)?,
            token,
            backend,
            cleanup,
        })
    }
    async fn command(
        &self,
        root: &Path,
        label: &str,
        command: PlaylistCommand,
    ) -> Result<PlaylistMutation> {
        let permit_command = if let PlaylistCommand::Tracks {
            id,
            revision,
            change: TrackChange::Clear,
            ..
        } = &command
        {
            PlaylistCommand::Tracks {
                id: id.clone(),
                revision: revision.clone(),
                change: TrackChange::Delete,
                ids: self.service.detail(&self.token, id).await?.tracks,
            }
        } else {
            command.clone()
        };
        self.backend.permit(Permit::command(
            label,
            &permit_command,
            self.token.uid.as_deref().unwrap(),
        )?)?;
        let result = self.service.execute(&self.token, command).await;
        self.backend.disarm()?;
        let mut record = result?;
        for _ in 0..3 {
            if !record.phase.pending() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            record = self
                .service
                .reconcile(&self.token, &record.operation_id)
                .await?;
        }
        save(
            root,
            &format!("stage-{label}.json"),
            &serde_json::to_value(&record)?,
        )?;
        if !matches!(
            record.phase,
            PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
        ) {
            return Err(Error::MutationUncertain);
        }
        Ok(record)
    }
}
pub async fn exercise(
    root: &Path,
    credentials: Arc<Credentials>,
    profile: ClientProfile,
    context: Arc<RequestContext>,
) -> Result<Value> {
    let _lock = OperationLock::acquire(root, "probe", "playlist-workflow")?;
    let run = Run::open(root, credentials, profile, context).await?;
    let baseline = run.service.list(&run.token).await?;
    let owner = run.token.uid.clone().unwrap();
    let favorite = baseline
        .playlists
        .iter()
        .find(|p| p.owner == owner && p.favorite)
        .ok_or(Error::State("own favorite playlist is missing"))?;
    let detail = run.service.detail(&run.token, &favorite.id).await?;
    if detail.tracks.len() < 3 || detail.has_local {
        return Err(Error::State("three normal source tracks required"));
    }
    let mut seed = [0; 8];
    SystemEntropyClock.fill(&mut seed)?;
    let suffix = format!("{:016x}", u64::from_be_bytes(seed));
    let plan = Plan {
        owner,
        name: format!("CumPc{suffix}"),
        renamed: format!("CumEdit{suffix}"),
        tracks: detail.tracks[..3].to_vec(),
        baseline,
    };
    if plan
        .baseline
        .playlists
        .iter()
        .any(|p| p.name == plan.name || p.name == plan.renamed)
    {
        return Err(Error::State("proof name collision"));
    }
    save(root, "playlist-plan.json", &serde_json::to_value(&plan)?)?;
    let outcome = workflow::execute(root, &plan, &run).await;
    run.backend.disarm()?;
    if let Err(e) = &outcome {
        save(
            root,
            "playlist-workflow-error.json",
            &json!({"error":e.to_string()}),
        )?;
    }
    let after = run.service.list(&run.token).await?;
    let restored =
        serde_json::to_value(&after.playlists)? == serde_json::to_value(&plan.baseline.playlists)?;
    save(
        root,
        "playlist-final-list.json",
        &serde_json::to_value(&after)?,
    )?;
    let report = json!({"ok":outcome.is_ok()&&restored,"workflowPassed":outcome.is_ok(),"baselineRestored":restored,"businessWrites":run.backend.write_count()?,"cleanupRequired":!restored,"stages":outcome.as_ref().ok(),"error":outcome.as_ref().err().map(ToString::to_string)});
    save(root, "playlist-result.json", &report)?;
    if outcome.is_err() || !restored {
        return Err(Error::MutationUncertain);
    }
    Ok(report)
}
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))?;
    f.write_all(&serde_json::to_vec_pretty(value)?)?;
    f.sync_all()?;
    Ok(())
}
