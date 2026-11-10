//! Search song-table download authority. A playback or drag receipt is never
//! accepted here; a normal download uses its own verified PC cookie scope.
use super::{MusicClient, SearchResultQuery};
use crate::catalog::search::results::SearchChannel;
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionToken,
        resources::{AccountResources, ResourceKind},
    },
    client::{downloads::GuardedSongBatch, dto::DownloadResult},
    download::service::SongBatchAdmission,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
    storage::PlaybackDownloadKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_PAGES: usize = 192;
const MAX_ROWS: usize = 100;
const PAGE_LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchGroupDownloadRight {
    Ordinary,
    CopyrightBlocked,
}

impl SearchGroupDownloadRight {
    pub(super) fn observe(song: &Value, id: &str) -> Option<Self> {
        let rights = &song["privilege"];
        if !rights.is_object()
            || (!rights["id"].is_null() && identifier(&rights["id"]).ok().as_deref() != Some(id))
            || rights["fee"].as_i64() != Some(0)
            || ["payed", "cs", "bd"]
                .iter()
                .any(|key| js_truthy(&rights[*key]))
            || [
                "programId",
                "radio",
                "audio",
                "mainTrackId",
                "offlineId",
                "filename",
                "localTrack",
                "onlineTrackId",
                "onlineTrack",
                "isOfflineDownOnline",
            ]
            .iter()
            .any(|key| js_truthy(&song[*key]))
        {
            return None;
        }
        if rights["st"].as_i64()? < 0 {
            // M35.j is a negative status check, independent of play bitrate.
            Some(Self::CopyrightBlocked)
        } else if rights["dl"].as_u64().is_some_and(|value| value > 0) {
            Some(Self::Ordinary)
        } else {
            None
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchGroupDownloadRow {
    pub(super) receipt: String,
    pub(super) right: SearchGroupDownloadRight,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchGroupDownloadTarget {
    receipt: String,
    track_id: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SearchGroupDownloadCommand {
    Prepare {
        anchor: SearchGroupDownloadTarget,
        rows: Vec<SearchGroupDownloadTarget>,
    },
    Start {
        receipt: String,
    },
    Status {
        receipt: String,
    },
    Cancel {
        receipt: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchGroupDownloadDecision {
    Ready,
    PlaylistRequired,
    MembershipUnknown,
    DirectoryRequired,
    AnchorUnavailable,
    Unavailable,
}

#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SearchGroupDownloadReply {
    Prepared {
        epoch: String,
        decision: SearchGroupDownloadDecision,
        receipt: Option<String>,
        eligible_count: usize,
        omitted_count: usize,
    },
    Accepted {
        result: DownloadResult,
    },
    Pending,
    NotStarted,
    Cancelled,
}

#[derive(PartialEq, Eq)]
struct QueryScope {
    keyword: String,
    correct: bool,
    channel: Option<SearchChannel>,
}
pub(super) struct Page {
    receipt: String,
    owner: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
    scope: QueryScope,
    offset: usize,
    at: Instant,
    rows: HashMap<String, Option<SearchGroupDownloadRight>>,
}
impl Page {
    pub(super) fn row(&self, id: &str) -> Option<SearchGroupDownloadRow> {
        Some(SearchGroupDownloadRow {
            receipt: self.receipt.clone(),
            right: self.rows.get(id).copied().flatten()?,
        })
    }
    fn live(&self) -> bool {
        self.at.elapsed() < PAGE_LIFETIME
    }
}

struct Observed {
    owner: SessionToken,
    api: Arc<dyn Api>,
    revision: String,
    anchor: String,
    rows: Vec<(String, SearchGroupDownloadRight)>,
    pages: Vec<Arc<Page>>,
}
struct Plan {
    receipt: String,
    observed: Arc<Observed>,
    songs: Arc<[String]>,
    busy: AtomicBool,
    committed: OnceLock<Vec<String>>,
}
struct Lease(Arc<Plan>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

#[derive(Default)]
struct State {
    pages: VecDeque<Arc<Page>>,
    ticket: u64,
    plan: Option<Arc<Plan>>,
}
#[derive(Default)]
pub(super) struct SearchGroupDownloads(Mutex<State>);

impl SearchGroupDownloads {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: String,
        query: &SearchResultQuery,
        candidates: Vec<(String, Option<SearchGroupDownloadRight>)>,
    ) -> Result<Option<Arc<Page>>> {
        let mut rows = HashMap::new();
        for (id, right) in candidates {
            rows.entry(id)
                .and_modify(|previous| {
                    if *previous != right {
                        *previous = None;
                    }
                })
                .or_insert(right);
        }
        let scope = QueryScope {
            keyword: query.keyword.clone(),
            correct: query.correct,
            channel: query.channel,
        };
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.pages.retain(|page| {
            page.owner == *token
                && page.live()
                && (page.scope != scope || page.offset != query.offset)
        });
        if rows.values().all(Option::is_none) {
            return Ok(None);
        }
        if state.pages.len() >= MAX_PAGES {
            state.pages.pop_front();
        }
        let page = Arc::new(Page {
            receipt: uuid::Uuid::new_v4().simple().to_string(),
            owner: token.clone(),
            api: Arc::downgrade(api),
            revision,
            scope,
            offset: query.offset,
            at: Instant::now(),
            rows,
        });
        state.pages.push_back(page.clone());
        Ok(Some(page))
    }

    fn begin(
        &self,
        owner: SessionToken,
        api: Arc<dyn Api>,
        revision: String,
        anchor: SearchGroupDownloadTarget,
        targets: Vec<SearchGroupDownloadTarget>,
    ) -> Result<(u64, Arc<Observed>)> {
        const INVALID: Error = Error::InvalidInput("invalid search download selection");
        if !(2..=MAX_ROWS).contains(&targets.len()) || !targets.contains(&anchor) {
            return Err(INVALID);
        }
        let api_ref = Arc::downgrade(&api);
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let mut pages: Vec<Arc<Page>> = Vec::new();
        let mut rows = Vec::with_capacity(targets.len());
        let mut seen = BTreeSet::new();
        for target in targets {
            if !valid_receipt(&target.receipt) || !seen.insert(target.track_id.clone()) {
                return Err(INVALID);
            }
            let page = state
                .pages
                .iter()
                .find(|page| {
                    page.receipt == target.receipt
                        && page.owner == owner
                        && page.live()
                        && page.revision == revision
                        && page.api.ptr_eq(&api_ref)
                })
                .ok_or(Error::StaleResponse)?;
            if pages.first().is_some_and(|first| first.scope != page.scope) {
                return Err(INVALID);
            }
            let right = page
                .rows
                .get(&target.track_id)
                .copied()
                .flatten()
                .ok_or(INVALID)?;
            rows.push((target.track_id, right));
            if !pages.iter().any(|candidate| Arc::ptr_eq(candidate, page)) {
                pages.push(page.clone());
            }
        }
        if state
            .plan
            .as_ref()
            .is_some_and(|plan| plan.busy.load(Ordering::Acquire))
        {
            return Err(Error::MutationBusy);
        }
        state.ticket = state
            .ticket
            .checked_add(1)
            .ok_or(Error::State("search download generation exhausted"))?;
        state.plan = None;
        Ok((
            state.ticket,
            Arc::new(Observed {
                owner,
                api,
                revision,
                anchor: anchor.track_id,
                rows,
                pages,
            }),
        ))
    }

    fn current(&self, observed: &Observed) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if observed
            .pages
            .iter()
            .all(|page| page.live() && state.pages.iter().any(|stored| Arc::ptr_eq(stored, page)))
        {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        }
    }

    fn publish(&self, ticket: u64, observed: Arc<Observed>, songs: Vec<String>) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.ticket != ticket {
            return Err(Error::StaleResponse);
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        state.plan = Some(Arc::new(Plan {
            receipt: receipt.clone(),
            observed,
            songs: songs.into(),
            busy: AtomicBool::new(false),
            committed: OnceLock::new(),
        }));
        Ok(receipt)
    }

    fn use_plan(&self, owner: &SessionToken, receipt: &str, start: bool) -> Result<Arc<Plan>> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let plan = state
            .plan
            .as_ref()
            .filter(|plan| &plan.observed.owner == owner && plan.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        if start && plan.busy.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        Ok(plan.clone())
    }

    fn cancel(&self, owner: &SessionToken, receipt: &str) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let plan = state
            .plan
            .as_ref()
            .filter(|plan| &plan.observed.owner == owner && plan.receipt == receipt)
            .ok_or(Error::StaleResponse)?;
        if plan.busy.load(Ordering::Acquire) || plan.committed.get().is_some() {
            return Err(Error::MutationBusy);
        }
        state.plan = None;
        Ok(())
    }
}

fn valid_receipt(receipt: &str) -> bool {
    receipt.len() == 32 && receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn authorization(session: &AccountSession, observed: &Observed) -> Result<()> {
    session.with_verified_profile(&observed.owner, &observed.api, |_, _| Ok(()))?;
    if observed.api.song_card_download_scope_revision()?.as_deref() != Some(&observed.revision) {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

fn accepted(epoch: &str, plan: &Plan, ids: Vec<String>) -> SearchGroupDownloadReply {
    SearchGroupDownloadReply::Accepted {
        result: DownloadResult {
            epoch: epoch.into(),
            operation_id: Some(plan.receipt.clone()),
            accepted: ids,
            ..Default::default()
        },
    }
}

impl MusicClient {
    pub async fn search_group_download(
        &self,
        epoch: &str,
        command: SearchGroupDownloadCommand,
    ) -> Result<SearchGroupDownloadReply> {
        use SearchGroupDownloadCommand as C;
        use SearchGroupDownloadReply as R;
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let host = self.search.group_downloads.clone();
        match command {
            C::Prepare { anchor, rows } => {
                self.prepare_search_group_download(epoch, token, anchor, rows)
                    .await
            }
            C::Cancel { receipt } => {
                host.cancel(&token, &receipt)?;
                Ok(R::Cancelled)
            }
            C::Status { receipt } => {
                let plan = host.use_plan(&token, &receipt, false)?;
                let busy = plan.busy.load(Ordering::Acquire);
                self.session.check_generation(&token)?;
                Ok(match plan.committed.get() {
                    Some(ids) => accepted(epoch, &plan, ids.clone()),
                    None if busy => R::Pending,
                    None => R::NotStarted,
                })
            }
            C::Start { receipt } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                let plan = lease.0.clone();
                let observed = plan.observed.clone();
                let login = self.auth.pending_guard();
                login()?;
                authorization(&self.session, &observed)?;
                host.current(&observed)?;
                if let Some(ids) = plan.committed.get() {
                    return Ok(accepted(epoch, &plan, ids.clone()));
                }
                let local = self.local.context().await?;
                let session = self.session.clone();
                let songs = plan.songs.clone();
                let check_songs = songs.clone();
                let anchor = observed.anchor.clone();
                let options = self.download_quality_preferences(epoch)?.options;
                let result = self
                    .start_guarded_song_batch(
                        epoch,
                        GuardedSongBatch {
                            receipt,
                            admission: SongBatchAdmission::LikedAnchor(anchor),
                            songs,
                            options,
                        },
                        move || {
                            login()?;
                            authorization(&session, &observed)?;
                            host.current(&observed)?;
                            let ids = check_songs
                                .iter()
                                .map(String::as_str)
                                .chain([observed.anchor.as_str()])
                                .collect();
                            let lock = local.library.hold_unmatched_online_songs(
                                &session,
                                &observed.owner,
                                &ids,
                            )?;
                            authorization(&session, &observed)?;
                            host.current(&observed)?;
                            login()?;
                            Ok(lock)
                        },
                        move |ids| {
                            let _ = lease.0.committed.set(ids.to_vec());
                            drop(lease);
                        },
                    )
                    .await?;
                self.session.check_generation(&token)?;
                Ok(R::Accepted { result })
            }
        }
    }

    async fn prepare_search_group_download(
        &self,
        epoch: &str,
        token: SessionToken,
        anchor: SearchGroupDownloadTarget,
        rows: Vec<SearchGroupDownloadTarget>,
    ) -> Result<SearchGroupDownloadReply> {
        use SearchGroupDownloadDecision as D;
        let login = self.auth.pending_guard();
        login()?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let revision = api
            .song_card_download_scope_revision()?
            .ok_or(Error::StaleResponse)?;
        let host = &self.search.group_downloads;
        let (ticket, observed) = host.begin(token, api, revision, anchor, rows)?;
        let local = self.local.context().await?;
        let session = self.session.clone();
        let candidate = observed.clone();
        let (songs, decision) = tokio::task::spawn_blocking(move || {
            let ids = candidate.rows.iter().map(|(id, _)| id.as_str()).collect();
            let matched =
                local
                    .library
                    .matched_online_song_ids(&session, &candidate.owner, &ids)?;
            let store = session.persistent_store()?;
            let mut songs = Vec::new();
            let decision = session.with_token(&candidate.owner, |owner| {
                store.read_snapshot(|tx| {
                    for (id, right) in &candidate.rows {
                        let tasks =
                            tx.playback_download_rows(owner, id, PlaybackDownloadKind::Song)?;
                        let complete = tasks.iter().any(|row| row.status == "complete");
                        if (complete || matched.contains(id.as_str())) && id == &candidate.anchor {
                            return Ok(D::AnchorUnavailable);
                        }
                        if *right == SearchGroupDownloadRight::Ordinary
                            && !matched.contains(id.as_str())
                            && !tasks.iter().any(|row| row.status != "cancelled")
                        {
                            songs.push(id.clone());
                        }
                    }
                    if songs.is_empty() {
                        return Ok(D::Unavailable);
                    }
                    let (liked, pending) = AccountResources::membership_in(
                        tx,
                        owner,
                        ResourceKind::Liked,
                        &candidate.anchor,
                    )?;
                    Ok(
                        match liked
                            .filter(|_| !pending.is_some_and(|record| record.phase.pending()))
                        {
                            Some(true) => D::Ready,
                            Some(false) => D::PlaylistRequired,
                            None => D::MembershipUnknown,
                        },
                    )
                })
            })?;
            Ok::<_, Error>((songs, decision))
        })
        .await
        .map_err(|_| Error::State("search download preparation stopped"))??;
        let decision = if decision == D::Ready
            && !self.configured_download_directory_available(epoch).await?
        {
            D::DirectoryRequired
        } else {
            decision
        };
        login()?;
        authorization(&self.session, &observed)?;
        host.current(&observed)?;
        let eligible_count = if decision == D::AnchorUnavailable {
            0
        } else {
            songs.len()
        };
        let omitted_count = observed.rows.len() - eligible_count;
        let receipt = if decision == D::Ready {
            Some(host.publish(ticket, observed, songs)?)
        } else {
            None
        };
        Ok(SearchGroupDownloadReply::Prepared {
            epoch: epoch.into(),
            decision,
            receipt,
            eligible_count,
            omitted_count,
        })
    }
}
