//! The Recently Played source for the shared song download workflow.
use super::*;
use crate::{
    account::resources::{AccountResources, ResourceKind},
    catalog::CatalogApi,
    client::{
        downloads::HistoryDownloadSources,
        dto::HistoryCollectTarget,
        history::song_download::{
            HistorySongDownloadProjection, HistorySongDownloadSource, download_picker_collectable,
            row_hidden,
        },
    },
    history::{HistoryKind, HistoryService, HistorySnapshot},
    library::LocalLibrary,
};

pub(super) enum PrepareTarget {
    Batch(HistorySelectionScope),
    Row(HistoryRowMenuTarget),
}

impl PrepareTarget {
    fn revision(&self) -> &str {
        match self {
            Self::Batch(target) => &target.revision,
            Self::Row(target) => &target.revision,
        }
    }

    fn project<'a>(
        &self,
        snapshot: &'a HistorySnapshot,
    ) -> Result<HistorySongDownloadProjection<'a>> {
        match self {
            Self::Batch(target) => HistorySongDownloadProjection::project(target, snapshot),
            Self::Row(target) => HistorySongDownloadProjection::project_row(target, snapshot),
        }
    }
}

pub(super) struct HistorySource {
    pub(super) revision: String,
    pub(super) selection_count: usize,
    pub(super) collection: Vec<String>,
    pub(super) singleton_anchor_supported: bool,
    pub(super) row: Option<HistoryRowSource>,
    offline: Option<HistoryDownloadSources>,
    at: Instant,
}

pub(super) struct HistoryRowSource {
    library: LocalLibrary,
    pub(super) known_local_count: usize,
    pub(super) unresolved: Vec<String>,
    pub(super) unsupported: bool,
}

impl Observed {
    pub(super) fn history_batch_source(&self) -> Option<HistoryBatchSource> {
        let Source::History(source) = &self.source else {
            return None;
        };
        Some(HistoryBatchSource {
            revision: source.revision.clone(),
            row_clicked: source.row.as_ref().map(|_| self.anchor.clone()),
            offline: source
                .offline
                .as_ref()
                .map(|offline| offline.task_ids().clone())
                .unwrap_or_default(),
        })
    }

    pub(super) fn history_row(&self) -> Option<&HistoryRowSource> {
        let Source::History(source) = &self.source else {
            return None;
        };
        source.row.as_ref()
    }

    pub(super) fn history_current(&self, session: &Arc<AccountSession>) -> Result<()> {
        let Source::History(source) = &self.source else {
            return Err(Error::StaleResponse);
        };
        if source.at.elapsed() >= PAGE_LIFETIME {
            return Err(Error::StaleResponse);
        }
        if HistoryService::new(session.clone())?
            .snapshot_for(&self.owner, HistoryKind::Song)?
            .revision
            != source.revision
        {
            return Err(Error::QueueChanged);
        }
        if let Some(offline) = &source.offline {
            offline.require()?;
        }
        Ok(())
    }
}

impl Plan {
    /// Recheck before a new remote picker write. Reconciliation of an
    /// already-dispatched write does not run this as a new admission.
    pub(super) fn check_history_row_downloads(&self, session: &AccountSession) -> Result<()> {
        let observed = &self.observed;
        let Some(row) = observed.history_row() else {
            return Ok(());
        };
        let wanted = self
            .songs
            .iter()
            .chain(self.collection.iter())
            .map(String::as_str)
            .chain(std::iter::once(observed.anchor.as_str()))
            .collect();
        let _local = row
            .library
            .hold_unmatched_online_songs(session, &observed.owner, &wanted)?;
        let store = session.persistent_store()?;
        session.with_token(&observed.owner, |owner| {
            store.read_snapshot(|tx| {
                if tx
                    .playback_download_rows(owner, &observed.anchor, PlaybackDownloadKind::Song)?
                    .iter()
                    .any(|task| task.status == "complete")
                {
                    return Err(Error::QueueChanged);
                }
                for song in self.songs.iter() {
                    let active = tx
                        .playback_download_rows(owner, song, PlaybackDownloadKind::Song)?
                        .iter()
                        .filter(|task| task.status != "cancelled")
                        .count();
                    match self.existing.get(song) {
                        Some(task) if active == 1 => task.check(tx, owner)?,
                        None if active == 0 => (),
                        _ => return Err(Error::QueueChanged),
                    }
                }
                let admitted: BTreeSet<_> = self.songs.iter().map(String::as_str).collect();
                for song in self
                    .collection
                    .iter()
                    .filter(|song| !admitted.contains(song.as_str()))
                {
                    if tx
                        .playback_download_rows(owner, song, PlaybackDownloadKind::Song)?
                        .iter()
                        .any(|task| task.status == "complete")
                    {
                        return Err(Error::QueueChanged);
                    }
                }
                Ok(())
            })
        })
    }
}

fn observed_right(song: &Value, id: &str) -> Option<RowRights> {
    RowRights::observe_with_source(song, id, true)
}

fn needs_privilege(song: &Value, id: &str) -> bool {
    if observed_right(song, id)
        .is_some_and(|right| right.right == SearchGroupDownloadRight::CopyrightBlocked)
    {
        return false;
    }
    let p = &song["privilege"];
    p.is_null() || p.is_object() && ["fee", "st", "dl"].iter().any(|field| p[*field].is_null())
}

fn row_anchor_payment(song: &Value, id: &str) -> Option<bool> {
    let rights = song["privilege"].as_object()?;
    if rights
        .get("id")
        .is_some_and(|value| !value.is_null() && identifier(value).ok().as_deref() != Some(id))
    {
        return None;
    }
    let raw = rights.get("st");
    let normalized = rights.get("status");
    if raw.zip(normalized).is_some_and(|(a, b)| a != b) {
        return None;
    }
    let status = raw.or(normalized)?.as_i64()?;
    // M21:258–264 resolves copyright before the fee gate. The positive
    // download bitrate already belongs to the sole survivor (M27.e), so
    // the independent clicked item needs no download bitrate of its own.
    if status < 0 {
        return Some(true);
    }
    match rights.get("fee")?.as_u64()? {
        0 => Some(true),
        _ => match rights.get("payed") {
            None | Some(Value::Null) => Some(false),
            Some(mask) => Some(mask.as_u64()? > 0),
        },
    }
}

fn rejected(
    epoch: &str,
    selected: usize,
    decision: SearchGroupDownloadDecision,
) -> SearchGroupDownloadReply {
    SearchGroupDownloadReply::Prepared {
        epoch: epoch.into(),
        decision,
        receipt: None,
        eligible_count: 0,
        omitted_count: selected,
        purchased_qualities: vec![],
        existing_purchased_quality: None,
    }
}

impl MusicClient {
    pub(super) async fn refresh_history_download_anchor(&self, observed: &Observed) -> Result<()> {
        if observed.history_revision().is_none() {
            return Ok(());
        }
        let resources = AccountResources::new(self.session.clone())?;
        let (liked, pending) =
            resources.membership_for(&observed.owner, ResourceKind::Liked, &observed.anchor)?;
        if liked.is_none() && !pending.is_some_and(|record| record.phase.pending()) {
            self.auth.pending_guard()()?;
            authorization(&self.session, observed)?;
            observed.history_current(&self.session)?;
            resources
                .refresh_for(&observed.owner, ResourceKind::Liked)
                .await?;
            self.auth.pending_guard()()?;
            authorization(&self.session, observed)?;
            observed.history_current(&self.session)?;
        }
        Ok(())
    }

    pub(super) async fn prepare_history_group_download(
        &self,
        epoch: &str,
        token: SessionToken,
        target: PrepareTarget,
    ) -> Result<SearchGroupDownloadReply> {
        let login = self.auth.pending_guard();
        login()?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let revision = api
            .song_card_download_scope_revision()?
            .ok_or(Error::StaleResponse)?;
        let history = HistoryService::new(self.session.clone())?;
        let mut snapshot = history.snapshot_for(&token, HistoryKind::Song)?;
        let projection = target.project(&snapshot)?;
        let candidates = match &target {
            PrepareTarget::Batch(target) => HistoryCollectTarget::Selected {
                revision: target.revision.clone(),
                ids: target.ids.clone(),
            }
            .download_candidates(&snapshot)?,
            PrepareTarget::Row(_) => projection.row_download_candidates(),
        };
        let offline = self.resolve_history_catalogue_downloads(epoch, &mut snapshot, candidates)?;
        let ticket = {
            let mut state = self
                .search
                .group_downloads
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            SearchGroupDownloads::begin_plan(&mut state)?
        };
        let check = || {
            login()?;
            self.session
                .with_verified_profile(&token, &api, |_, _| Ok(()))?;
            if api.song_card_download_scope_revision()?.as_deref() != Some(&revision)
                || self
                    .search
                    .group_downloads
                    .0
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?
                    .ticket
                    != ticket
            {
                return Err(Error::StaleResponse);
            }
            if history.snapshot_for(&token, HistoryKind::Song)?.revision != target.revision() {
                return Err(Error::QueueChanged);
            }
            if let Some(offline) = &offline {
                offline.require()?;
            }
            Ok(())
        };
        let projection = target.project(&snapshot)?;
        let row_menu = matches!(&target, PrepareTarget::Row(_));
        let mut row = if row_menu {
            Some(HistoryRowSource {
                library: self.local.context().await?.library.clone(),
                known_local_count: 0,
                unresolved: Vec::new(),
                unsupported: false,
            })
        } else {
            None
        };
        let anchor_supported = |projection: &HistorySongDownloadProjection<'_>| {
            if row_menu {
                projection.row_anchor_source() == HistorySongDownloadSource::Catalog
            } else {
                projection
                    .rows
                    .first()
                    .is_some_and(|row| row.source == HistorySongDownloadSource::Catalog)
            }
        };
        if !anchor_supported(&projection) {
            check()?;
            return Ok(rejected(
                epoch,
                projection.rows.len(),
                SearchGroupDownloadDecision::Unavailable,
            ));
        }
        if let Some(row) = &row {
            let anchor = projection.anchor.id.clone();
            let library = row.library.clone();
            let session = self.session.clone();
            let owner = token.clone();
            let unavailable = tokio::task::spawn_blocking(move || {
                row_hidden(&library, &session, &owner, &anchor)
            })
            .await
            .map_err(|_| Error::State("history song menu inspection stopped"))??;
            check()?;
            if unavailable {
                return Ok(rejected(
                    epoch,
                    projection.rows.len(),
                    SearchGroupDownloadDecision::AnchorUnavailable,
                ));
            }
        }
        let mut wanted: BTreeSet<_> = projection
            .rows
            .iter()
            .filter(|row| {
                row.source == HistorySongDownloadSource::Catalog
                    && needs_privilege(&row.record.resource, &row.record.id)
            })
            .map(|row| row.record.id.clone())
            .collect();
        if row_menu
            && row_anchor_payment(&projection.anchor.resource, &projection.anchor.id).is_none()
        {
            wanted.insert(projection.anchor.id.clone());
        }
        if !wanted.is_empty() {
            let mut rights = BTreeMap::new();
            let ids: Vec<_> = wanted.iter().cloned().collect();
            for privilege in CatalogApi::new(api.clone()).privileges(&ids, None).await? {
                let id = identifier(&privilege["id"])?;
                if !wanted.contains(&id) || rights.insert(id, privilege).is_some() {
                    return Err(Error::Response("unexpected history download privilege"));
                }
            }
            check()?;
            for row in &mut snapshot.entries {
                if let Some(privilege) = rights.remove(&row.id) {
                    row.resource["privilege"] = privilege;
                }
            }
        }
        let projection = target.project(&snapshot)?;
        if !anchor_supported(&projection) {
            check()?;
            return Ok(rejected(
                epoch,
                projection.rows.len(),
                SearchGroupDownloadDecision::Unavailable,
            ));
        }
        let mut rows = Vec::new();
        for selected in &projection.rows {
            let id = &selected.record.id;
            if selected.source == HistorySongDownloadSource::Catalog {
                if let Some(mut rights) = observed_right(&selected.record.resource, id) {
                    if row_menu {
                        rights.collectable = download_picker_collectable(&selected.record.resource);
                    }
                    rows.push((id.clone(), rights));
                } else if let Some(row) = &mut row {
                    row.unresolved.push(id.clone());
                }
            } else if let Some(row) = &mut row {
                if selected.source == HistorySongDownloadSource::Local {
                    row.known_local_count += 1;
                } else {
                    row.unsupported = true;
                }
            }
        }
        if rows.is_empty() && !row_menu {
            check()?;
            return Ok(rejected(
                epoch,
                projection.rows.len(),
                SearchGroupDownloadDecision::Unavailable,
            ));
        }
        let anchor = projection.anchor.id.clone();
        let anchor_privilege = &projection.anchor.resource["privilege"];
        // M3085:3866 calls H.x(originalAnchor, false) after checking the
        // survivor's download bitrate. M23/M21 still route an unpaid positive
        // fee to checkout. A positive dl does not waive that anchor check.
        let singleton_anchor_supported = if row_menu {
            row_anchor_payment(&projection.anchor.resource, &anchor) == Some(true)
        } else {
            observed_right(&projection.anchor.resource, &anchor).is_some()
                && (anchor_privilege["fee"].as_i64() == Some(0)
                    || anchor_privilege["st"]
                        .as_i64()
                        .is_some_and(|status| status < 0)
                    || Offer::from_privilege(anchor_privilege).is_some())
        };
        check()?;
        let collection = if row_menu {
            Vec::new()
        } else {
            projection.playlist_song_ids()?
        };
        let observed = Arc::new(Observed {
            owner: token,
            api,
            revision,
            anchor,
            rows,
            source: Source::History(Box::new(HistorySource {
                revision: target.revision().to_owned(),
                selection_count: projection.rows.len(),
                collection,
                singleton_anchor_supported,
                row,
                offline,
                at: Instant::now(),
            })),
        });
        self.prepare_observed_group_download(epoch, ticket, observed)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::row_anchor_payment;
    use serde_json::json;

    #[test]
    fn original_row_anchor_payment_does_not_require_the_survivors_download_bitrate() {
        let mut clicked = json!({"privilege":{"id":71,"st":0,"fee":4,"payed":2,"dl":0}});
        assert_eq!(row_anchor_payment(&clicked, "71"), Some(true));
        clicked["privilege"]["payed"] = json!(0);
        assert_eq!(row_anchor_payment(&clicked, "71"), Some(false));
        clicked["privilege"]["payed"] = json!("malformed");
        assert_eq!(row_anchor_payment(&clicked, "71"), None);
        assert_eq!(row_anchor_payment(&clicked, "72"), None);
        clicked["privilege"]["st"] = json!(-100);
        assert_eq!(row_anchor_payment(&clicked, "71"), Some(true));
        clicked["privilege"]["status"] = json!(0);
        assert_eq!(row_anchor_payment(&clicked, "71"), None);
    }
}
