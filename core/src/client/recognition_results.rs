//! Account-authorized catalog projection for a completed native recognition.
//! There is no IPC, media source resolver, or device/transport entry here.

use super::{
    dto::{AccountData, ArtistReference},
    mapping,
};
use crate::{
    Error, Result,
    account::{AccountSession, session::SessionToken},
    catalog::{CatalogApi, SongBatch},
    protocol::api::identifier,
    song_recognition::request::{Completed, Generation, MAX_SONG_IDS, ResultStatus},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::watch;

const PRIVILEGE_DELAY: Duration = Duration::from_millis(500);
const READ_BUDGET: Duration = Duration::from_secs(8);
const MAX_TEXT_CHARS: usize = 512;
const MAX_ARTISTS: usize = 32;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecognitionTrack {
    id: String,
    metadata: Option<RecognitionMetadata>,
    privilege: PrivilegeObservation,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecognitionMetadata {
    name: String,
    artists: Vec<ArtistReference>,
    album: String,
    duration_ms: u64,
    fee: i64,
}

/// A catalog observation, never a grant to start audio or bypass source checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PrivilegeObservation {
    PlaybackIndicated,
    TrialIndicated,
    NoPlaybackIndication,
    Unknown,
}

enum Phase {
    Capturing,
    Matched(Arc<[String]>),
    Retired,
}

struct Scope {
    owner: SessionToken,
    generation: Generation,
    revision: u64,
    phase: Phase,
}

struct Lease {
    owner: SessionToken,
    generation: Generation,
    revision: u64,
    ids: Arc<[String]>,
}

/// Pair one coordinator with one persistent scheduler. The capture entry must
/// snapshot the account token before starting the scheduler, and revoke on
/// dismissal even when the scheduler has already completed.
pub(super) struct RecognitionResults {
    session: Arc<AccountSession>,
    state: Mutex<Option<Scope>>,
    changed: watch::Sender<()>,
}

impl RecognitionResults {
    pub(super) fn new(session: Arc<AccountSession>) -> Self {
        Self {
            session,
            state: Mutex::new(None),
            changed: watch::channel(()).0,
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<Scope>>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }

    pub(super) fn begin(&self, owner: &SessionToken, generation: Generation) -> Result<()> {
        self.session.with_token(owner, |_| {
            let mut state = self.lock()?;
            if state
                .as_ref()
                .is_some_and(|current| !generation.is_after(current.generation))
            {
                return Err(Error::StaleResponse);
            }
            *state = Some(Scope {
                owner: owner.clone(),
                generation,
                revision: 0,
                phase: Phase::Capturing,
            });
            self.changed.send_replace(());
            Ok(())
        })
    }

    pub(super) fn complete(&self, completed: &Completed) -> Result<()> {
        let generation = completed.generation();
        let owner = self
            .lock()?
            .as_ref()
            .filter(|scope| scope.generation == generation)
            .ok_or(Error::StaleResponse)?
            .owner
            .clone();
        self.session.with_token(&owner, |_| {
            let mut state = self.lock()?;
            let scope = Self::scope_mut(&mut state, generation)?;
            match scope.phase {
                Phase::Capturing => {}
                Phase::Retired => return Err(Error::Cancelled),
                Phase::Matched(_) => return Err(Error::StaleResponse),
            }
            scope.phase = match completed.status() {
                ResultStatus::Matched { song_ids }
                    if !song_ids.is_empty() && song_ids.len() <= MAX_SONG_IDS =>
                {
                    Phase::Matched(Arc::from(song_ids.as_slice()))
                }
                _ => Phase::Retired,
            };
            self.changed.send_replace(());
            Ok(())
        })
    }

    pub(super) fn cancel(&self, generation: Generation) -> Result<()> {
        let mut state = self.lock()?;
        let scope = Self::scope_mut(&mut state, generation)?;
        scope.phase = Phase::Retired;
        self.changed.send_replace(());
        Ok(())
    }

    /// The caller drives entry or an explicit entitlement refresh. A later
    /// read supersedes an in-flight read for the same recognition result.
    pub(super) async fn read(
        &self,
        generation: Generation,
    ) -> Result<AccountData<Vec<RecognitionTrack>>> {
        let mut scope_changes = self.changed.subscribe();
        let mut account_changes = self.session.subscribe()?;
        let lease = self.reserve(generation)?;
        self.check(&lease)?;
        let mut seen = HashSet::new();
        let detail_ids = lease
            .ids
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        let work = async {
            tokio::time::sleep(PRIVILEGE_DELAY).await;
            self.check(&lease)?;
            tokio::time::timeout(
                READ_BUDGET,
                self.session.run_for(&lease.owner, |api, _| async {
                    let catalog = CatalogApi::new(api);
                    // Recognition currently holds candidate identities only.
                    // Reuse v3 details for metadata; their inline privileges
                    // are deliberately not used in the projection.
                    tokio::try_join!(
                        catalog.privileges(&lease.ids, None),
                        catalog.songs(&detail_ids, MAX_SONG_IDS)
                    )
                }),
            )
            .await
            .map_err(|_| Error::State("recognition catalog read timed out"))?
            .map_err(catalog_failure)
        };
        let response = tokio::select! {
            biased;
            _ = account_changes.wait_for(|owner| owner != &lease.owner) => {
                return Err(Error::StaleResponse);
            }
            result = self.wait_for_revocation(&lease, &mut scope_changes) => return result,
            result = work => result,
        };
        self.check(&lease)?;
        let (privileges, batch) = response?;
        let tracks = project(&lease.ids, detail_ids.len(), privileges, batch)?;
        // Commit only while both the account and the exact operation are
        // still current. Consumers must also fence later UI dispatch.
        self.session.with_token(&lease.owner, |_| {
            self.check_scope(&lease)?;
            Ok(AccountData {
                epoch: lease.owner.generation.to_string(),
                value: tracks,
            })
        })
    }

    fn scope_mut(state: &mut Option<Scope>, generation: Generation) -> Result<&mut Scope> {
        state
            .as_mut()
            .filter(|scope| scope.generation == generation)
            .ok_or(Error::StaleResponse)
    }

    fn reserve(&self, generation: Generation) -> Result<Lease> {
        let mut state = self.lock()?;
        let scope = Self::scope_mut(&mut state, generation)?;
        let Phase::Matched(ids) = &scope.phase else {
            return match scope.phase {
                Phase::Retired => Err(Error::Cancelled),
                _ => Err(Error::State("recognition has no completed candidates")),
            };
        };
        scope.revision = scope
            .revision
            .checked_add(1)
            .ok_or(Error::State("recognition read sequence exhausted"))?;
        self.changed.send_replace(());
        Ok(Lease {
            owner: scope.owner.clone(),
            generation,
            revision: scope.revision,
            ids: ids.clone(),
        })
    }

    fn check(&self, lease: &Lease) -> Result<()> {
        self.session
            .with_token(&lease.owner, |_| self.check_scope(lease))
    }

    fn check_scope(&self, lease: &Lease) -> Result<()> {
        let state = self.lock()?;
        let scope = state.as_ref().ok_or(Error::StaleResponse)?;
        if scope.generation != lease.generation
            || scope.owner != lease.owner
            || scope.revision != lease.revision
        {
            return Err(Error::StaleResponse);
        }
        if matches!(scope.phase, Phase::Retired) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    async fn wait_for_revocation<T>(
        &self,
        lease: &Lease,
        changes: &mut watch::Receiver<()>,
    ) -> Result<T> {
        loop {
            self.check_scope(lease)?;
            changes.changed().await.map_err(|_| Error::Cancelled)?;
        }
    }
}

fn project(
    ids: &[String],
    detail_count: usize,
    privileges: Vec<Value>,
    batch: SongBatch,
) -> Result<Vec<RecognitionTrack>> {
    if batch.songs.len() != detail_count {
        return Err(Error::Response("recognition catalog rows not aligned"));
    }
    let candidates: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut observations = HashMap::with_capacity(candidates.len());
    for privilege in privileges {
        let id = identifier(&privilege["id"])?;
        if candidates.contains(id.as_str()) {
            // Official M/21 uses last privilege row by identity.
            observations.insert(id, observe(&privilege));
        }
    }
    let metadata: HashMap<_, _> = batch
        .songs
        .into_iter()
        .flatten()
        .filter_map(|row| mapping::track(&row).ok())
        .map(|track| {
            let metadata = RecognitionMetadata {
                name: bounded_text(&track.name),
                artists: track
                    .artist_refs
                    .into_iter()
                    .take(MAX_ARTISTS)
                    .map(|artist| ArtistReference {
                        id: artist.id.filter(|id| id.len() <= 64),
                        name: bounded_text(&artist.name),
                    })
                    .collect(),
                album: bounded_text(&track.album),
                duration_ms: track.duration_ms,
                fee: track.fee,
            };
            (track.id, metadata)
        })
        .collect();
    Ok(ids
        .iter()
        .map(|id| RecognitionTrack {
            id: id.clone(),
            metadata: metadata.get(id).cloned(),
            privilege: observations
                .get(id)
                .copied()
                .unwrap_or(PrivilegeObservation::Unknown),
        })
        .collect())
}

// AccountSession sees the original error (including 301 policy) before this
// boundary. Keep useful failure kinds without retaining a raw response body.
fn catalog_failure(error: Error) -> Error {
    match error.root() {
        Error::StaleResponse => Error::StaleResponse,
        Error::Cancelled => Error::Cancelled,
        Error::LoginDeferred => Error::LoginDeferred,
        Error::Service { code, .. } => code
            .as_i64()
            .or_else(|| code.as_str()?.parse::<i64>().ok())
            .map_or(Error::Response("recognition catalog rejected"), |code| {
                Error::service("recognition catalog", json!(code), Value::Null)
            }),
        Error::HttpStatus { status, .. } => Error::HttpStatus {
            status: *status,
            endpoint: None,
            body: None,
        },
        Error::Transport(_) | Error::RequestNotSent(_) => Error::Transport(String::new()),
        _ => Error::Response("recognition catalog unavailable"),
    }
}

fn observe(privilege: &Value) -> PrivilegeObservation {
    match (
        privilege["pl"].as_i64(),
        privilege["flag"]
            .as_i64()
            .is_some_and(|flag| flag & (1 << 10) != 0),
    ) {
        (Some(pl), _) if pl > 0 => PrivilegeObservation::PlaybackIndicated,
        (_, true) => PrivilegeObservation::TrialIndicated,
        (Some(0), false) => PrivilegeObservation::NoPlaybackIndication,
        _ => PrivilegeObservation::Unknown,
    }
}

fn bounded_text(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests;
