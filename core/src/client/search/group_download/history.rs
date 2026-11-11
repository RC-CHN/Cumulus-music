//! The Recently Played source for the shared song download workflow.
use super::*;
use crate::{
    account::resources::{AccountResources, ResourceKind},
    catalog::CatalogApi,
    client::{
        downloads::HistoryDownloadSources,
        dto::HistoryCollectTarget,
        history::song_download::{HistorySongDownloadProjection, HistorySongDownloadSource},
    },
    history::{HistoryKind, HistoryService},
};

pub(super) struct HistorySource {
    pub(super) revision: String,
    pub(super) selection_count: usize,
    pub(super) collection: Vec<String>,
    pub(super) singleton_anchor_supported: bool,
    offline: Option<HistoryDownloadSources>,
    at: Instant,
}

impl Observed {
    pub(super) fn history_batch_source(&self) -> Option<HistoryBatchSource> {
        let Source::History(source) = &self.source else {
            return None;
        };
        Some(HistoryBatchSource {
            revision: source.revision.clone(),
            offline: source
                .offline
                .as_ref()
                .map(|offline| offline.task_ids().clone())
                .unwrap_or_default(),
        })
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

fn unavailable(epoch: &str, selected: usize) -> SearchGroupDownloadReply {
    SearchGroupDownloadReply::Prepared {
        epoch: epoch.into(),
        decision: SearchGroupDownloadDecision::Unavailable,
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
        target: HistorySelectionScope,
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
        HistorySongDownloadProjection::project(&target, &snapshot)?;
        let collection_target = HistoryCollectTarget::Selected {
            revision: target.revision.clone(),
            ids: target.ids.clone(),
        };
        let candidates = collection_target.download_candidates(&snapshot)?;
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
            if history.snapshot_for(&token, HistoryKind::Song)?.revision != target.revision {
                return Err(Error::QueueChanged);
            }
            if let Some(offline) = &offline {
                offline.require()?;
            }
            Ok(())
        };
        let projection = HistorySongDownloadProjection::project(&target, &snapshot)?;
        if projection
            .rows
            .first()
            .is_none_or(|row| row.source != HistorySongDownloadSource::Catalog)
        {
            check()?;
            return Ok(unavailable(epoch, projection.rows.len()));
        }
        let wanted: BTreeSet<_> = projection
            .rows
            .iter()
            .filter(|row| {
                row.source == HistorySongDownloadSource::Catalog
                    && needs_privilege(&row.record.resource, &row.record.id)
            })
            .map(|row| row.record.id.clone())
            .collect();
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
        let projection = HistorySongDownloadProjection::project(&target, &snapshot)?;
        if projection
            .rows
            .first()
            .is_none_or(|row| row.source != HistorySongDownloadSource::Catalog)
        {
            check()?;
            return Ok(unavailable(epoch, projection.rows.len()));
        }
        let rows: Vec<_> = projection
            .rows
            .iter()
            .filter(|row| row.source == HistorySongDownloadSource::Catalog)
            .filter_map(|row| {
                let id = &row.record.id;
                observed_right(&row.record.resource, id).map(|right| (id.clone(), right))
            })
            .collect();
        if rows.is_empty() {
            check()?;
            return Ok(unavailable(epoch, projection.rows.len()));
        }
        let anchor = projection.anchor.id.clone();
        let anchor_privilege = &projection.anchor.resource["privilege"];
        // M3085:3866 calls H.x(originalAnchor, false) after checking the
        // survivor's download bitrate. M23/M21 still route an unpaid positive
        // fee to checkout. A positive dl does not waive that anchor check.
        let singleton_anchor_supported = observed_right(&projection.anchor.resource, &anchor)
            .is_some()
            && (anchor_privilege["fee"].as_i64() == Some(0)
                || anchor_privilege["st"]
                    .as_i64()
                    .is_some_and(|status| status < 0)
                || Offer::from_privilege(anchor_privilege).is_some());
        check()?;
        let collection = projection.playlist_song_ids()?;
        let observed = Arc::new(Observed {
            owner: token,
            api,
            revision,
            anchor,
            rows,
            source: Source::History(Box::new(HistorySource {
                revision: target.revision,
                selection_count: projection.rows.len(),
                collection,
                singleton_anchor_supported,
                offline,
                at: Instant::now(),
            })),
        });
        self.prepare_observed_group_download(epoch, ticket, observed)
            .await
    }
}
