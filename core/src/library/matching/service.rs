use super::{
    FileMetadataProbe, MatchAction, MatchHint, MatchJob, MatchPhase, MatchQuery, MatchingApi,
    MetadataProbe,
    state::{CANDIDATES, CURRENT, JOBS, key},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    catalog::CatalogApi,
    library::{
        LibraryItem, ListOptions, LocalLibrary, LocalMapping, MappingSource, catalog::LocalQuery,
        index::MAPS,
    },
    protocol::api::identifier,
    storage::{OperationLock, StateStore, StateTransaction},
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub struct LocalMatching {
    session: Arc<AccountSession>,
    token: SessionToken,
    library: LocalLibrary,
    store: Arc<StateStore>,
    probe: Arc<dyn MetadataProbe>,
}
enum Outcome {
    Matched(Value, MappingSource),
    Unmatched,
    Skipped,
}
struct Batch {
    job: MatchJob,
    items: Vec<LibraryItem>,
}
fn read_job(tx: &StateTransaction<'_>, uid: &str) -> Result<Option<MatchJob>> {
    Ok(tx
        .get(uid, JOBS, CURRENT)?
        .map(serde_json::from_value)
        .transpose()?)
}
fn write_job(tx: &mut StateTransaction<'_>, uid: &str, job: &MatchJob) -> Result<()> {
    tx.put(uid, JOBS, CURRENT, &serde_json::to_value(job)?)
}
fn next_revision(job: &mut MatchJob) -> Result<()> {
    job.revision = job
        .revision
        .checked_add(1)
        .ok_or(Error::State("matching revision exhausted"))?;
    Ok(())
}
fn require_job(tx: &StateTransaction<'_>, uid: &str, expected: &MatchJob) -> Result<MatchJob> {
    read_job(tx, uid)?
        .filter(|j| j.id == expected.id && j.revision == expected.revision && j.phase.runnable())
        .ok_or(Error::Cancelled)
}
fn still_current(item: &LibraryItem, current: &BTreeMap<String, LibraryItem>) -> bool {
    current
        .get(&item.id)
        .is_some_and(|now| now.revision == item.revision && now.available())
        && LocalLibrary::check_current(item).is_ok()
}

impl LocalMatching {
    pub fn new(session: Arc<AccountSession>, library: LocalLibrary) -> Result<Self> {
        Self::with_probe(session, library, Arc::new(FileMetadataProbe))
    }
    pub fn with_probe(
        session: Arc<AccountSession>,
        library: LocalLibrary,
        probe: Arc<dyn MetadataProbe>,
    ) -> Result<Self> {
        let token = session.token()?;
        session.with_token(&token, |_| Ok(()))?;
        let store = library.state_store();
        if store.directory().canonicalize()?
            != session.persistent_store()?.directory().canonicalize()?
        {
            return Err(Error::InvalidInput(
                "matching library and account stores differ",
            ));
        }
        Ok(Self {
            session,
            token,
            library,
            store,
            probe,
        })
    }
    pub fn token(&self) -> &SessionToken {
        &self.token
    }
    pub fn state(&self) -> Result<Option<MatchJob>> {
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| read_job(tx, uid))
        })
    }
    fn current_items(&self) -> Result<BTreeMap<String, LibraryItem>> {
        Ok(self
            .library
            .list(ListOptions::default())?
            .into_iter()
            .map(|i| (i.id.clone(), i))
            .collect())
    }

    /// Captures the entire filtered list, independent of display pagination.
    /// Already mapped current revisions are excluded as in M/171 activeMatch.
    pub fn start(&self, query: &LocalQuery) -> Result<MatchJob> {
        let _index = self.library.lock()?;
        let snapshot = self.library.catalog(query)?;
        if snapshot.items.len() > 50_000 {
            return Err(Error::InvalidInput("matching selection exceeds 50000"));
        }
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                if read_job(tx, uid)?.is_some_and(|job| !job.phase.terminal()) {
                    return Err(Error::MutationBusy);
                }
                let mut items = Vec::new();
                for item in snapshot.items.into_iter().filter(LibraryItem::available) {
                    let mapping: Option<LocalMapping> = tx
                        .get(uid, MAPS, &item.id)?
                        .map(serde_json::from_value)
                        .transpose()?;
                    if mapping.is_none_or(|m| m.revision != item.revision) {
                        items.push(item);
                    }
                }
                // Old candidate rows are private to this workflow and may be pruned
                // only after the previous job is terminal.
                for old in tx.values(uid, CANDIDATES)?.keys() {
                    tx.delete(uid, CANDIDATES, old)?;
                }
                let job = MatchJob {
                    id: uuid::Uuid::new_v4().to_string(),
                    phase: if items.is_empty() {
                        MatchPhase::Complete
                    } else {
                        MatchPhase::Queued
                    },
                    total: items.len(),
                    processed: 0,
                    matched: 0,
                    unmatched: 0,
                    skipped: 0,
                    revision: 0,
                    error: None,
                };
                for (i, item) in items.iter().enumerate() {
                    tx.put(uid, CANDIDATES, &key(&job, i), &serde_json::to_value(item)?)?;
                }
                write_job(tx, uid, &job)?;
                Ok(job)
            })
        })
    }

    pub fn control(&self, id: &str, action: MatchAction) -> Result<MatchJob> {
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                let mut job = read_job(tx, uid)?
                    .filter(|j| j.id == id)
                    .ok_or(Error::State("unknown matching job"))?;
                if job.phase.terminal() {
                    return Ok(job);
                }
                job.phase = match action {
                    MatchAction::Pause => MatchPhase::Paused,
                    MatchAction::Resume => MatchPhase::Queued,
                    MatchAction::Cancel => MatchPhase::Cancelled,
                };
                job.error = None;
                next_revision(&mut job)?;
                write_job(tx, uid, &job)?;
                Ok(job)
            })
        })
    }

    /// A reopened host explicitly pauses an interrupted queue. Read-only HTTP
    /// can be retried on resume; completed batches are never repeated.
    pub fn recover(&self) -> Result<Option<MatchJob>> {
        let _worker = self.worker_lock()?;
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                let Some(mut job) = read_job(tx, uid)? else {
                    return Ok(None);
                };
                if job.phase.runnable() {
                    job.phase = MatchPhase::Paused;
                    next_revision(&mut job)?;
                    write_job(tx, uid, &job)?;
                }
                Ok(Some(job))
            })
        })
    }
    fn worker_lock(&self) -> Result<OperationLock> {
        self.session.with_token(&self.token, |uid| {
            OperationLock::acquire(self.store.directory(), uid, "local-matching-worker")
        })
    }
    fn prepare(&self) -> Result<Option<Batch>> {
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                let Some(mut job) = read_job(tx, uid)?.filter(|j| j.phase.runnable()) else {
                    return Ok(None);
                };
                if job.processed >= job.total {
                    return Err(Error::State("invalid matching cursor"));
                }
                let items = (job.processed..(job.processed + 50).min(job.total))
                    .map(|i| {
                        serde_json::from_value(
                            tx.get(uid, CANDIDATES, &key(&job, i))?
                                .ok_or(Error::State("missing matching candidate"))?,
                        )
                        .map_err(Error::from)
                    })
                    .collect::<Result<Vec<_>>>()?;
                job.phase = MatchPhase::Running;
                write_job(tx, uid, &job)?;
                Ok(Some(Batch { job, items }))
            })
        })
    }
    fn inspect(&self, batch: &Batch) -> Result<(Vec<Outcome>, Vec<MatchQuery>)> {
        self.session.with_token(&self.token, |_| Ok(()))?;
        let current = self.current_items()?;
        let mut outcomes = Vec::new();
        let mut queries = Vec::new();
        for item in &batch.items {
            if !still_current(item, &current) {
                outcomes.push(Outcome::Skipped);
                continue;
            }
            match self.probe.inspect(item) {
                Ok(MatchHint::Song(song)) => {
                    identifier(&song["id"])?;
                    outcomes.push(Outcome::Matched(song, MappingSource::FileMetadata));
                }
                Ok(MatchHint::Absent) => {
                    queries.push(MatchQuery::from_item(item)?);
                    outcomes.push(Outcome::Unmatched);
                }
                Ok(MatchHint::Invalid) | Err(Error::Media(_)) => outcomes.push(Outcome::Unmatched),
                Err(Error::Io(_)) => outcomes.push(Outcome::Skipped),
                Err(error) => return Err(error),
            }
        }
        Ok((outcomes, queries))
    }
    fn finish(&self, batch: Batch, outcomes: Vec<Outcome>) -> Result<MatchJob> {
        let _index = self.library.lock()?;
        let current = self.current_items()?;
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                let mut job = require_job(tx, uid, &batch.job)?;
                for (i, (item, outcome)) in batch.items.iter().zip(outcomes).enumerate() {
                    let existing: Option<LocalMapping> = tx
                        .get(uid, MAPS, &item.id)?
                        .map(serde_json::from_value)
                        .transpose()?;
                    if !still_current(item, &current)
                        || existing.is_some_and(|m| m.revision == item.revision)
                    {
                        job.skipped += 1;
                    } else {
                        match outcome {
                            Outcome::Skipped => job.skipped += 1,
                            Outcome::Unmatched => job.unmatched += 1,
                            Outcome::Matched(song, source) => {
                                let mapping = LocalMapping {
                                    song_id: identifier(&song["id"])?,
                                    revision: item.revision.clone(),
                                    song: Some(song),
                                    source: Some(source),
                                };
                                tx.put(uid, MAPS, &item.id, &serde_json::to_value(mapping)?)?;
                                job.matched += 1;
                            }
                        }
                    }
                    tx.delete(uid, CANDIDATES, &key(&batch.job, batch.job.processed + i))?;
                }
                job.processed += batch.items.len();
                job.phase = if job.processed == job.total {
                    MatchPhase::Complete
                } else {
                    MatchPhase::Queued
                };
                job.error = None;
                next_revision(&mut job)?;
                write_job(tx, uid, &job)?;
                Ok(job)
            })
        })
    }
    fn failed(&self, expected: &MatchJob, error: &Error) -> Result<()> {
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                let mut job = match require_job(tx, uid, expected) {
                    Ok(j) => j,
                    Err(Error::Cancelled) => return Ok(()),
                    Err(e) => return Err(e),
                };
                job.phase = MatchPhase::Failed;
                job.error = Some(
                    match error.root() {
                        Error::Service { .. } | Error::HttpStatus { .. } => "service_rejected",
                        Error::Transport(_) => "transport",
                        Error::Response(_) => "invalid_response",
                        Error::Storage(_) | Error::Io(_) => "storage_or_file",
                        _ => "matching_failed",
                    }
                    .into(),
                );
                next_revision(&mut job)?;
                write_job(tx, uid, &job)
            })
        })
    }

    pub async fn step(&self) -> Result<Option<MatchJob>> {
        let this = self.clone();
        let (lock, batch) = tokio::task::spawn_blocking(move || {
            let lock = this.worker_lock()?;
            Ok::<_, Error>((lock, this.prepare()?))
        })
        .await
        .map_err(|_| Error::State("matching worker stopped"))??;
        let _lock = lock;
        let Some(batch) = batch else { return Ok(None) };
        let expected = batch.job.clone();
        let result = self.execute(batch).await;
        if let Err(error) = &result {
            if matches!(error, Error::Cancelled | Error::StaleResponse) {
                return result.map(Some);
            }
            // A write failure may also prevent this failure marker. The durable
            // Running cursor is still recoverable; never advance it in memory.
            self.failed(&expected, error)?;
        }
        result.map(Some)
    }
    async fn execute(&self, batch: Batch) -> Result<MatchJob> {
        let this = self.clone();
        let (batch, mut outcomes, queries) = tokio::task::spawn_blocking(move || {
            let (outcomes, queries) = this.inspect(&batch)?;
            Ok::<_, Error>((batch, outcomes, queries))
        })
        .await
        .map_err(|_| Error::State("matching metadata worker stopped"))??;
        if !queries.is_empty() {
            self.session.with_token(&self.token, |uid| {
                self.store
                    .transaction(|tx| require_job(tx, uid, &batch.job).map(|_| ()))
            })?;
            let mut matched = self
                .session
                .run_for(&self.token, |api, _| async {
                    MatchingApi::new(api).match_batch(&queries).await
                })
                .await?;
            for (item, outcome) in batch.items.iter().zip(&mut outcomes) {
                if let Some(song) = matched.remove(&item.id) {
                    *outcome = Outcome::Matched(song, MappingSource::Search);
                }
            }
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.finish(batch, outcomes))
            .await
            .map_err(|_| Error::State("matching commit worker stopped"))?
    }

    /// Manual selection uses authoritative song details and a captured local
    /// revision. No local metadata tags or remote account resources are edited.
    pub async fn bind(
        &self,
        id: &str,
        expected: &crate::library::LocalRevision,
        song_id: &Value,
    ) -> Result<LocalMapping> {
        let song_id = identifier(song_id)?;
        let this = self.clone();
        let local_id = id.to_owned();
        let revision = expected.clone();
        let _worker = tokio::task::spawn_blocking(move || {
            let worker = this.worker_lock()?;
            let item = this.library.item(&local_id)?;
            if item.revision != revision {
                return Err(Error::QueueChanged);
            };
            LocalLibrary::check_current(&item)?;
            if this.state()?.is_some_and(|j| j.phase.runnable()) {
                return Err(Error::MutationBusy);
            };
            Ok::<_, Error>(worker)
        })
        .await
        .map_err(|_| Error::State("manual mapping worker stopped"))??;
        let song = self
            .session
            .run_for(&self.token, |api, _| async {
                CatalogApi::new(api)
                    .songs(&[Value::String(song_id)], 1)
                    .await?
                    .songs
                    .into_iter()
                    .next()
                    .flatten()
                    .ok_or(Error::Response("selected song unavailable"))
            })
            .await?;
        let this = self.clone();
        let id = id.to_owned();
        let expected = expected.clone();
        tokio::task::spawn_blocking(move || {
            this.edit_mapping(&id, &expected, Some(song))?
                .ok_or(Error::State("missing manual mapping"))
        })
        .await
        .map_err(|_| Error::State("manual mapping commit stopped"))?
    }
    pub fn clear(&self, id: &str, expected: &crate::library::LocalRevision) -> Result<()> {
        let _worker = self.worker_lock()?;
        self.edit_mapping(id, expected, None).map(|_| ())
    }
    fn edit_mapping(
        &self,
        id: &str,
        expected: &crate::library::LocalRevision,
        song: Option<Value>,
    ) -> Result<Option<LocalMapping>> {
        let _index = self.library.lock()?;
        let item = self.library.item(id)?;
        if &item.revision != expected {
            return Err(Error::QueueChanged);
        };
        LocalLibrary::check_current(&item)?;
        self.session.with_token(&self.token, |uid| {
            self.store.transaction(|tx| {
                if read_job(tx, uid)?.is_some_and(|j| j.phase.runnable()) {
                    return Err(Error::MutationBusy);
                };
                let mapping = song
                    .map(|song| {
                        Ok::<_, Error>(LocalMapping {
                            song_id: identifier(&song["id"])?,
                            revision: item.revision,
                            song: Some(song),
                            source: Some(MappingSource::Manual),
                        })
                    })
                    .transpose()?;
                match &mapping {
                    Some(m) => tx.put(uid, MAPS, id, &serde_json::to_value(m)?)?,
                    None => tx.delete(uid, MAPS, id)?,
                };
                Ok(mapping)
            })
        })
    }
}
