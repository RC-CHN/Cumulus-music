//! Narrow reversible write proof. No frontend-controlled mutation parameters.
mod guard;
mod recovery;
use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken, credentials::Credentials},
    playlist::{
        PlaylistApi, PlaylistCommand, PlaylistList, PlaylistMutation, PlaylistPhase,
        PlaylistService,
    },
    protocol::{context::RequestContext, http::*},
    storage::StateStore,
    watchman::{EntropyClock, SystemEntropyClock},
};
use guard::{Backend, Scope};
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
    baseline: PlaylistList,
}
struct Run {
    backend: Arc<Backend>,
    service: PlaylistService,
    token: SessionToken,
    cleanup: Arc<MusicHttpClient>,
    initial_intents: usize,
}
impl Run {
    async fn open(
        root: &Path,
        name: &str,
        recovering: bool,
        credentials: Arc<Credentials>,
        profile: ClientProfile,
        context: Arc<RequestContext>,
    ) -> Result<Self> {
        let store = Arc::new(StateStore::open(root.join("write-state"))?);
        let backend = Arc::new(Backend::new(
            root.to_owned(),
            Scope {
                name: name.into(),
                owner: String::new(),
                id: None,
                create_sent: recovering,
                cleanup: false,
            },
        )?);
        let api = Arc::new(MusicHttpClient::with_backend(
            credentials.clone(),
            profile.clone(),
            RequestOptions {
                routing: RoutingPolicy::OfficialEapi,
                ..Default::default()
            },
            Some(context.clone()),
            backend.clone(),
        )?);
        let session = Arc::new(AccountSession::new(api, Some(store)));
        session.refresh().await?;
        let token = session.token()?;
        let owner = token
            .uid
            .clone()
            .ok_or(Error::State("no account for write proof"))?;
        backend.scope.lock().map_err(|_| Error::LockPoisoned)?.owner = owner;
        let cleanup = Arc::new(MusicHttpClient::with_backend(
            credentials,
            profile,
            RequestOptions::default(),
            Some(context),
            backend.clone(),
        )?);
        let initial_intents = backend.creates.load(std::sync::atomic::Ordering::SeqCst)
            + backend.deletes.load(std::sync::atomic::Ordering::SeqCst);
        Ok(Self {
            backend,
            service: PlaylistService::new(session)?,
            token,
            cleanup,
            initial_intents,
        })
    }
}
pub async fn exercise(
    root: &Path,
    credentials: Arc<Credentials>,
    profile: ClientProfile,
    context: Arc<RequestContext>,
) -> Result<Value> {
    let mut random = [0; 8];
    SystemEntropyClock.fill(&mut random)?;
    let name = format!("CumEapi{:016x}", u64::from_be_bytes(random));
    let run = Run::open(root, &name, false, credentials, profile, context).await?;
    let baseline = run.service.list(&run.token).await?;
    if baseline.playlists.iter().any(|p| p.name == name) {
        return Err(Error::State("proof name already exists"));
    }
    let plan = Plan {
        owner: run.token.uid.clone().unwrap(),
        name: name.clone(),
        baseline,
    };
    save_new(root, "write-plan.json", &serde_json::to_value(&plan)?)?;
    let result = run
        .service
        .execute(
            &run.token,
            PlaylistCommand::Create {
                name: name.clone(),
                private: true,
            },
        )
        .await;
    let record = match result {
        Ok(r) => r,
        Err(e) => {
            save_new(
                root,
                "write-error.json",
                &json!({"error":e.to_string(),"recovery":"read journal and list; never resend create"}),
            )?;
            return Err(e);
        }
    };
    finish(root, &plan, &run, Some(record)).await
}

async fn finish(
    root: &Path,
    plan: &Plan,
    run: &Run,
    mut record: Option<PlaylistMutation>,
) -> Result<Value> {
    let Run {
        backend,
        service,
        token,
        cleanup,
        initial_intents,
    } = run;
    let before: Vec<_> = plan
        .baseline
        .playlists
        .iter()
        .map(|p| p.id.clone())
        .collect();
    for _ in 0..3 {
        let Some(r) = record.as_ref().filter(|r| r.phase.pending()) else {
            break;
        };
        tokio::time::sleep(Duration::from_millis(500)).await;
        record = Some(service.reconcile(token, &r.operation_id).await?);
    }
    save_new(root, "write-create.json", &serde_json::to_value(&record)?)?;
    let current = service.list(token).await?;
    let candidates = current
        .playlists
        .iter()
        .filter(|p| {
            p.name == plan.name && p.owner == plan.owner && p.private && !before.contains(&p.id)
        })
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        return Err(Error::State("ambiguous proof-created resources; no delete"));
    }
    let mut cleanup_sent = false;
    if let Some(created) = candidates
        .first()
        .filter(|_| backend.deletes.load(std::sync::atomic::Ordering::SeqCst) == 0)
    {
        let snapshot = service.detail(token, &created.id).await?;
        if !snapshot.summary.private
            || snapshot.summary.owner != plan.owner
            || snapshot.summary.name != plan.name
            || snapshot.summary.favorite
            || !snapshot.tracks.is_empty()
            || before.contains(&snapshot.summary.id)
            || record.as_ref().is_none_or(|r| {
                r.phase != PlaylistPhase::Confirmed || r.playlist_id.as_ref() != Some(&created.id)
            })
        {
            return Err(Error::State("proof resource changed; cleanup refused"));
        }
        save_new(root, "write-owned.json", &serde_json::to_value(&snapshot)?)?;
        {
            let mut scope = backend.scope.lock().map_err(|_| Error::LockPoisoned)?;
            scope.id = Some(created.id.clone());
            scope.cleanup = true;
        }
        // Cleanup is planned as a separate, previously verified compatibility intent.
        // It is not an automatic protocol retry of the EAPI create or another write.
        let reply = PlaylistApi::new(cleanup.clone())
            .delete(&json!(created.id))
            .await;
        save_new(
            root,
            "write-cleanup.json",
            &match &reply {
                Ok(v) => json!({"acknowledged":true,"response":v}),
                Err(e) => json!({"acknowledged":false,"error":e.to_string()}),
            },
        )?;
        cleanup_sent = true;
        // A lost cleanup response is reconciled only through reads, never resent.
    }
    let mut restored = None;
    for i in 0..4 {
        let after = service.list(token).await?;
        let ids: Vec<_> = after.playlists.iter().map(|p| p.id.clone()).collect();
        save_new(
            root,
            &format!("write-readback-{i}.json"),
            &serde_json::to_value(&after)?,
        )?;
        if ids == before {
            restored = Some(after);
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let creates = backend.creates.load(std::sync::atomic::Ordering::SeqCst);
    let deletes = backend.deletes.load(std::sync::atomic::Ordering::SeqCst);
    let safe = restored.is_some() && record.as_ref().map_or(creates == 0, |r| !r.phase.pending());
    let report = json!({"completedAudit":safe,"officialCreateAccepted":record.as_ref().is_some_and(|r| r.phase==PlaylistPhase::Confirmed),"createPhase":record.as_ref().map(|r|r.phase),"createCodes":record.as_ref().map(|r|&r.codes),"cleanupSent":cleanup_sent,"baselineRestored":restored.is_some(),"createRequests":creates,"cleanupRequests":deletes,"businessWrites":creates+deletes-initial_intents,"totalWriteIntents":creates+deletes,"cleanupRequired":!safe});
    save_new(root, "write-result.json", &report)?;
    if !safe {
        return Err(Error::MutationUncertain);
    }
    Ok(report)
}
fn save_new(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
