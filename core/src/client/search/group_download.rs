//! Search song-table download authority. A playback or drag receipt is never
//! accepted here; a normal download uses its own verified PC cookie scope.
mod destination;
mod history;
mod purchased;
use super::{MusicClient, SearchResultQuery};
use crate::catalog::search::results::SearchChannel;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::{
        downloads::{
            GuardedSongBatch,
            purchased::{Offer, purchased_qualities},
        },
        dto::{
            AccountData, DownloadResult, HistorySelectionScope, PlaylistCollectPage,
            PlaylistCollectSort,
        },
    },
    download::service::{ExistingSongAdmission, HistoryBatchSource, SongBatchAdmission},
    membership::Quality,
    playlist::GroupDownloadPlaylistProof,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
    storage::PlaybackDownloadKind,
};
pub use destination::SearchGroupDownloadPlaylistResult;
pub use destination::creation::SearchGroupDownloadCreation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
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
    Purchased,
    CopyrightBlocked,
}

impl SearchGroupDownloadRight {
    fn observe(song: &Value, id: &str, history: bool) -> Option<(Self, Option<Offer>)> {
        let rights = &song["privilege"];
        if !rights.is_object()
            || (!rights["id"].is_null() && identifier(&rights["id"]).ok().as_deref() != Some(id))
            || ["cs", "bd"].iter().any(|key| js_truthy(&rights[*key]))
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
        let status = rights["st"].as_i64()?;
        let fee = rights["fee"].as_i64().filter(|fee| *fee >= 0);
        if status < 0 && (history || fee.is_some()) {
            // M35.j is a negative status check, independent of purchase or
            // play bitrate. This row cannot contribute a quality or task.
            Some((Self::CopyrightBlocked, None))
        } else if status < 0 {
            None
        } else if let Some(offer) = fee.and_then(|_| Offer::from_privilege(rights)) {
            Some((Self::Purchased, Some(offer)))
        } else if history
            && rights["payed"]
                .as_u64()
                .and_then(|mask| u32::try_from(mask).ok())
                .is_some_and(|mask| mask & 6 != 0)
        {
            // M3085 selects purchased quality from payed bits before native
            // task admission. Keep the known purchase even if dl/fee cannot
            // authorize a quality; don't silently dispatch an ordinary batch.
            Some((Self::Purchased, None))
        } else if (fee == Some(0) || history && fee.is_some())
            && !js_truthy(&rights["payed"])
            && rights["dl"].as_u64().is_some_and(|value| value > 0)
        {
            Some((Self::Ordinary, None))
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
    #[serde(skip)]
    pub(super) collectable: bool,
    #[serde(skip)]
    offer: Option<Offer>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct RowRights {
    right: SearchGroupDownloadRight,
    collectable: bool,
    offer: Option<Offer>,
}
impl RowRights {
    pub(super) fn observe(song: &Value, id: &str) -> Option<Self> {
        Self::observe_with_source(song, id, false)
    }

    fn observe_with_source(song: &Value, id: &str, history: bool) -> Option<Self> {
        let (right, offer) = SearchGroupDownloadRight::observe(song, id, history)?;
        // M21 converts subp to subPriv unchanged. The picker passes the
        // array as resource, so only M3085's second, truthy-or-positive
        // filter applies; ordinary multiselect collect's fallback does not.
        let raw = &song["privilege"]["subp"];
        let collectable = !js_truthy(raw)
            || raw
                .as_f64()
                .or_else(|| raw.as_str()?.trim().parse().ok())
                .is_some_and(|value| value.is_finite() && value > 0.0);
        Some(Self {
            right,
            collectable,
            offer,
        })
    }
    pub(super) fn row(self, receipt: String) -> SearchGroupDownloadRow {
        SearchGroupDownloadRow {
            receipt,
            right: self.right,
            collectable: self.collectable,
            offer: self.offer,
        }
    }
}
impl From<&SearchGroupDownloadRow> for RowRights {
    fn from(row: &SearchGroupDownloadRow) -> Self {
        Self {
            right: row.right,
            collectable: row.collectable,
            offer: row.offer,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchGroupDownloadTarget {
    receipt: String,
    track_id: String,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SearchGroupDownloadCommand {
    Prepare {
        anchor: SearchGroupDownloadTarget,
        rows: Vec<SearchGroupDownloadTarget>,
    },
    PrepareHistory {
        target: HistorySelectionScope,
    },
    Start {
        receipt: String,
    },
    ConfirmQuality {
        receipt: String,
        quality: Quality,
    },
    Status {
        receipt: String,
    },
    Cancel {
        receipt: String,
    },
    Destinations {
        receipt: String,
        sort: PlaylistCollectSort,
        page: u32,
    },
    Choose {
        receipt: String,
        playlist_id: String,
    },
    ChooseLiked {
        receipt: String,
        destination_receipt: String,
    },
    CreatePlaylist {
        receipt: String,
        name: String,
        private_playlist: bool,
    },
    CreationStatus {
        receipt: String,
        reconcile: bool,
    },
    AddCreated {
        receipt: String,
    },
    Reconcile {
        receipt: String,
    },
    StartFromPlaylist {
        receipt: String,
        continuation: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchGroupDownloadDecision {
    Ready,
    PurchasedQualityRequired,
    PurchasedSelectionUnavailable,
    PlaylistRequired,
    PlaylistUnavailable,
    MembershipUnknown,
    DirectoryRequired,
    AnchorUnavailable,
    AlreadyAvailable,
    CopyrightBlocked,
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
        #[serde(skip_serializing_if = "Vec::is_empty")]
        purchased_qualities: Vec<Quality>,
        #[serde(skip_serializing_if = "Option::is_none")]
        existing_purchased_quality: Option<Quality>,
    },
    QualityConfirmed {
        epoch: String,
        decision: SearchGroupDownloadDecision,
        receipt: Option<String>,
        quality: Quality,
    },
    ExistingQualityMismatch {
        epoch: String,
        receipt: String,
        required_quality: Quality,
    },
    Accepted {
        result: DownloadResult,
    },
    Destinations {
        page: AccountData<PlaylistCollectPage>,
        system_liked_receipt: Option<String>,
        can_create: bool,
    },
    Playlist {
        result: SearchGroupDownloadPlaylistResult,
    },
    Creation {
        result: SearchGroupDownloadCreation,
    },
    DirectoryRequired,
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
    rows: HashMap<String, Option<RowRights>>,
}
impl Page {
    pub(super) fn row(&self, id: &str) -> Option<SearchGroupDownloadRow> {
        Some(
            self.rows
                .get(id)
                .copied()
                .flatten()?
                .row(self.receipt.clone()),
        )
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
    rows: Vec<(String, RowRights)>,
    source: Source,
}

enum Source {
    Search(Vec<Arc<Page>>),
    History(Box<history::HistorySource>),
}

impl Observed {
    fn history_revision(&self) -> Option<&str> {
        match &self.source {
            Source::History(source) => Some(&source.revision),
            Source::Search(_) => None,
        }
    }

    fn selection_count(&self) -> usize {
        match &self.source {
            Source::History(source) => source.selection_count,
            Source::Search(_) => self.rows.len(),
        }
    }
}
struct PlanDraft {
    songs: Vec<String>,
    existing: BTreeMap<String, ExistingSongAdmission>,
    collection: Vec<String>,
    purpose: SearchGroupDownloadDecision,
    purchased_qualities: Vec<Quality>,
    existing_purchased_quality: Option<Quality>,
}
struct Plan {
    receipt: String,
    observed: Arc<Observed>,
    songs: Arc<[String]>,
    existing: Arc<BTreeMap<String, ExistingSongAdmission>>,
    collection: Arc<[String]>,
    purpose: SearchGroupDownloadDecision,
    purchased_qualities: Vec<Quality>,
    existing_purchased_quality: Option<Quality>,
    quality: OnceLock<purchased::Choice>,
    destination: Mutex<destination::Destination>,
    busy: AtomicBool,
    committed: OnceLock<Vec<String>>,
}

impl Plan {
    fn direct_admission(&self) -> SongBatchAdmission {
        let anchor = self.observed.anchor.clone();
        match self.observed.history_batch_source() {
            Some(source) => SongBatchAdmission::HistoryLikedAnchor { anchor, source },
            None => SongBatchAdmission::LikedAnchor(anchor),
        }
    }

    fn playlist_admission(&self, proof: GroupDownloadPlaylistProof) -> SongBatchAdmission {
        match self.observed.history_batch_source() {
            Some(source) => SongBatchAdmission::HistoryPlaylist { proof, source },
            None => SongBatchAdmission::SearchPlaylist {
                proof,
                anchor: self.observed.anchor.clone(),
            },
        }
    }
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
        candidates: Vec<(String, Option<RowRights>)>,
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
        let ticket = Self::begin_plan(&mut state)?;
        Ok((
            ticket,
            Arc::new(Observed {
                owner,
                api,
                revision,
                anchor: anchor.track_id,
                rows,
                source: Source::Search(pages),
            }),
        ))
    }

    fn begin_plan(state: &mut State) -> Result<u64> {
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
        Ok(state.ticket)
    }

    fn current(&self, session: &Arc<AccountSession>, observed: &Observed) -> Result<()> {
        let Source::Search(pages) = &observed.source else {
            return observed.history_current(session);
        };
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if pages
            .iter()
            .all(|page| page.live() && state.pages.iter().any(|stored| Arc::ptr_eq(stored, page)))
        {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        }
    }

    fn publish(&self, ticket: u64, observed: Arc<Observed>, draft: PlanDraft) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.ticket != ticket {
            return Err(Error::StaleResponse);
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        state.plan = Some(Arc::new(Plan {
            receipt: receipt.clone(),
            observed,
            songs: draft.songs.into(),
            existing: Arc::new(draft.existing),
            collection: draft.collection.into(),
            purpose: draft.purpose,
            purchased_qualities: draft.purchased_qualities,
            existing_purchased_quality: draft.existing_purchased_quality,
            quality: OnceLock::new(),
            destination: Mutex::default(),
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
            C::PrepareHistory { target } => {
                self.prepare_history_group_download(epoch, token, target)
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
                if lease.0.purpose != SearchGroupDownloadDecision::Ready {
                    return Err(Error::StaleResponse);
                }
                let songs = lease.0.songs.clone();
                let admission = lease.0.direct_admission();
                self.enqueue_search_download(epoch, lease, songs, admission, false)
                    .await
            }
            C::ConfirmQuality { receipt, quality } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.confirm_search_group_quality(epoch, lease, quality)
                    .await
            }
            C::Destinations {
                receipt,
                sort,
                page,
            } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_destinations(epoch, &lease, sort, page)
                    .await
            }
            C::Choose {
                receipt,
                playlist_id,
            } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_choose(
                    &lease,
                    destination::Choice::Regular(&playlist_id),
                )
                .await
            }
            C::ChooseLiked {
                receipt,
                destination_receipt,
            } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_choose(
                    &lease,
                    destination::Choice::SystemLiked(&destination_receipt),
                )
                .await
            }
            C::Reconcile { receipt } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_reconcile(&lease).await
            }
            C::CreatePlaylist {
                receipt,
                name,
                private_playlist,
            } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_creation(
                    &lease,
                    destination::creation::Request::Create {
                        name: &name,
                        private: private_playlist,
                    },
                )
                .await
            }
            C::CreationStatus { receipt, reconcile } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_creation(
                    &lease,
                    destination::creation::Request::Status { reconcile },
                )
                .await
            }
            C::AddCreated { receipt } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_creation(&lease, destination::creation::Request::Add)
                    .await
            }
            C::StartFromPlaylist {
                receipt,
                continuation,
            } => {
                let lease = Lease(host.use_plan(&token, &receipt, true)?);
                self.search_group_download_start_from_playlist(epoch, lease, &continuation)
                    .await
            }
        }
    }

    async fn enqueue_search_download(
        &self,
        epoch: &str,
        lease: Lease,
        songs: Arc<[String]>,
        admission: SongBatchAdmission,
        through_playlist: bool,
    ) -> Result<SearchGroupDownloadReply> {
        let plan = lease.0.clone();
        let observed = plan.observed.clone();
        let session = self.session.clone();
        let host = self.search.group_downloads.clone();
        let login = self.auth.pending_guard();
        let quality = plan.quality.get().map(|choice| choice.quality);
        if quality.is_some() == plan.purchased_qualities.is_empty() {
            return Err(Error::StaleResponse);
        }
        if let Some(quality) = quality
            && plan
                .existing
                .values()
                .any(|task| !task.has_bound_prefix() || task.options().quality != quality)
        {
            return Err(Error::StaleResponse);
        }
        let experiments = self.pc_experiments.clone();
        let system_liked = admission
            .playlist_proof()
            .is_some_and(|proof| proof.liked_origin.is_some());
        let created = admission
            .playlist_proof()
            .is_some_and(|proof| proof.creation.is_some());
        let check = move || {
            login()?;
            authorization(&session, &observed)?;
            purchased::check_quality(&experiments, &observed, quality)?;
            if system_liked {
                destination::authorize_liked(&observed)?;
            } else if through_playlist {
                destination::authorize_collection(&observed)?;
            }
            if created {
                destination::authorize_creation(&observed)?;
            }
            host.current(&session, &observed)
        };
        check()?;
        if let Some(ids) = plan.committed.get() {
            return Ok(accepted(epoch, &plan, ids.clone()));
        }
        let local = self.local.context().await?;
        let session = self.session.clone();
        let observed = plan.observed.clone();
        let check_songs = songs.clone();
        let mut options = self.download_quality_preferences(epoch)?.options;
        if let Some(quality) = quality {
            options.quality = quality;
        }
        let result = self
            .start_guarded_song_batch(
                epoch,
                GuardedSongBatch {
                    receipt: plan.receipt.clone(),
                    admission,
                    existing: plan.existing.clone(),
                    songs,
                    options,
                },
                move || {
                    check()?;
                    let anchor = observed
                        .history_revision()
                        .is_none()
                        .then_some(observed.anchor.as_str());
                    let ids = check_songs
                        .iter()
                        .map(String::as_str)
                        .chain(anchor)
                        .collect();
                    let lock = local.library.hold_unmatched_online_songs(
                        &session,
                        &observed.owner,
                        &ids,
                    )?;
                    check()?;
                    Ok(lock)
                },
                move |ids| {
                    let _ = lease.0.committed.set(ids.to_vec());
                    drop(lease);
                },
            )
            .await?;
        self.session.check_generation(&plan.observed.owner)?;
        Ok(SearchGroupDownloadReply::Accepted { result })
    }

    async fn prepare_search_group_download(
        &self,
        epoch: &str,
        token: SessionToken,
        anchor: SearchGroupDownloadTarget,
        rows: Vec<SearchGroupDownloadTarget>,
    ) -> Result<SearchGroupDownloadReply> {
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
        self.prepare_observed_group_download(epoch, ticket, observed)
            .await
    }

    async fn prepare_observed_group_download(
        &self,
        epoch: &str,
        ticket: u64,
        observed: Arc<Observed>,
    ) -> Result<SearchGroupDownloadReply> {
        use SearchGroupDownloadDecision as D;
        let host = &self.search.group_downloads;
        let login = self.auth.pending_guard();
        login()?;
        let local = self.local.context().await?;
        let session = self.session.clone();
        let candidate = observed.clone();
        let (songs, existing, collection, offers, decision, existing_purchased_quality) =
            tokio::task::spawn_blocking(move || {
                let ids = candidate.rows.iter().map(|(id, _)| id.as_str()).collect();
                let matched =
                    local
                        .library
                        .matched_online_song_ids(&session, &candidate.owner, &ids)?;
                let store = session.persistent_store()?;
                let mut songs = Vec::new();
                let mut existing = BTreeMap::new();
                let history = candidate.history_revision().is_some();
                let mut collection = match &candidate.source {
                    Source::History(source) => source.collection.clone(),
                    Source::Search(_) => Vec::new(),
                };
                let mut offers = Vec::new();
                let mut first_filter_count = 0;
                let mut already_available = 0;
                let mut copyright_blocked = 0;
                let mut unverified_purchase = false;
                let mut existing_quality = None;
                let decision = session.with_token(&candidate.owner, |owner| {
                    store.read_snapshot(|tx| {
                        for (id, right) in &candidate.rows {
                            let tasks =
                                tx.playback_download_rows(owner, id, PlaybackDownloadKind::Song)?;
                            let complete = tasks.iter().any(|row| row.status == "complete");
                            if complete || matched.contains(id.as_str()) {
                                if !history && id == &candidate.anchor {
                                    return Ok(D::AnchorUnavailable);
                                }
                                already_available += 1;
                                continue;
                            }
                            if right.right == SearchGroupDownloadRight::CopyrightBlocked {
                                copyright_blocked += 1;
                                continue;
                            }
                            // M/3085:3806–3841 counts completed/local/blocked
                            // filtering, not the later native task admission.
                            first_filter_count += 1;
                            if right.right == SearchGroupDownloadRight::Purchased
                                && right.offer.is_none()
                            {
                                unverified_purchase = true;
                                continue;
                            }
                            if !history && right.collectable {
                                collection.push(id.clone());
                            }
                            offers.extend(right.offer);
                            let mut current = tasks.iter().filter(|row| row.status != "cancelled");
                            if let Some(task) = current.next() {
                                if current.next().is_none()
                                    && let Some(previous) =
                                        ExistingSongAdmission::observe(tx, owner, id, &task.id)?
                                {
                                    existing.insert(id.clone(), previous);
                                    songs.push(id.clone());
                                }
                            } else {
                                songs.push(id.clone());
                            }
                        }
                        if unverified_purchase {
                            return Ok(D::PurchasedSelectionUnavailable);
                        }
                        if first_filter_count == 1
                            && let Source::History(source) = &candidate.source
                            && !source.singleton_anchor_supported
                        {
                            return Ok(D::Unavailable);
                        }
                        if !offers.is_empty() {
                            // A sole purchased survivor already passes the
                            // M/27 preflight (positive dl, no BD or cloud);
                            // observed anchors are either fee-free, paid or
                            // copyright-blocked, never the original fee prompt.
                            // The anchor remains independent of this list.
                            // No excluded task can masquerade as a first-filter
                            // omission. Exact quiescent old tasks may only use
                            // their common original tier with proven prefixes.
                            existing_quality = purchased::common_existing_quality(&existing);
                            let admissible = songs.len() == first_filter_count
                                && (existing.is_empty() || existing_quality.is_some());
                            return Ok(if admissible {
                                D::PurchasedQualityRequired
                            } else {
                                D::PurchasedSelectionUnavailable
                            });
                        }
                        if songs.is_empty() {
                            if history
                                && first_filter_count == 0
                                && already_available + copyright_blocked
                                    == candidate.selection_count()
                            {
                                return Ok(if already_available == 0 {
                                    D::CopyrightBlocked
                                } else {
                                    D::AlreadyAvailable
                                });
                            }
                            return Ok(D::Unavailable);
                        }
                        purchased::anchor_membership(tx, owner, &candidate.anchor)
                    })
                })?;
                Ok::<_, Error>((
                    songs,
                    existing,
                    collection,
                    offers,
                    decision,
                    existing_quality,
                ))
            })
            .await
            .map_err(|_| Error::State("search download preparation stopped"))??;
        let decision = if decision == D::MembershipUnknown && observed.history_revision().is_some()
        {
            self.refresh_history_download_anchor(&observed).await?;
            self.session.with_token(&observed.owner, |owner| {
                self.session
                    .persistent_store()?
                    .read_snapshot(|tx| purchased::anchor_membership(tx, owner, &observed.anchor))
            })?
        } else {
            decision
        };
        let qualities = if decision == D::PurchasedQualityRequired {
            purchased_qualities(offers, purchased::vivid(&self.pc_experiments, &observed)?)
        } else {
            Vec::new()
        };
        let decision = if decision == D::PurchasedQualityRequired
            && (qualities.is_empty()
                || existing_purchased_quality.is_some_and(|tier| !qualities.contains(&tier)))
        {
            D::PurchasedSelectionUnavailable
        } else if decision == D::Ready
            && !self.configured_download_directory_available(epoch).await?
        {
            D::DirectoryRequired
        } else if decision == D::PlaylistRequired
            && (collection.is_empty() || destination::authorize_collection(&observed).is_err())
        {
            D::PlaylistUnavailable
        } else {
            decision
        };
        login()?;
        authorization(&self.session, &observed)?;
        host.current(&self.session, &observed)?;
        let eligible_count = if matches!(decision, D::AnchorUnavailable | D::Unavailable) {
            0
        } else {
            songs.len()
        };
        let omitted_count = observed.selection_count() - eligible_count;
        let existing_purchased_quality = (decision == D::PurchasedQualityRequired)
            .then_some(existing_purchased_quality)
            .flatten();
        let receipt = if matches!(
            decision,
            D::Ready | D::PlaylistRequired | D::PurchasedQualityRequired
        ) {
            Some(host.publish(
                ticket,
                observed,
                PlanDraft {
                    songs,
                    existing,
                    collection,
                    purpose: decision,
                    purchased_qualities: qualities.clone(),
                    existing_purchased_quality,
                },
            )?)
        } else {
            None
        };
        Ok(SearchGroupDownloadReply::Prepared {
            epoch: epoch.into(),
            decision,
            receipt,
            eligible_count,
            omitted_count,
            purchased_qualities: qualities,
            existing_purchased_quality,
        })
    }
}
