//! The desktop batch route borrows the current detail, without reopening it.
use super::{MusicClient, ZonePlaylistSnapshot, playlist_track_page};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{
        dto::{AccountData, PageRequest, TrackPage},
        mapping,
    },
    playback::engine::Cancellation,
    protocol::api::identifier,
};
use serde_json::json;
use std::sync::Arc;

pub(in crate::client) struct ZoneBatchContext {
    pub snapshot: Arc<ZonePlaylistSnapshot>,
    pub guard: Cancellation,
}

impl MusicClient {
    pub(in crate::client) async fn zone_playlist_batch_context(
        &self,
        token: &SessionToken,
        receipt: &str,
        id: &str,
        revision: &str,
    ) -> Result<ZoneBatchContext> {
        let authority = self
            .vertical_zone_authority(&token.generation.to_string())
            .await?
            .filter(|authority| authority.token == *token)
            .ok_or(Error::StaleResponse)?;
        let snapshot = self
            .zone_playlist_visit(&authority, receipt, id, |visit| {
                visit
                    .snapshot
                    .as_ref()
                    .filter(|row| {
                        row.revision == revision && row.read_sequence == visit.read_sequence
                    })
                    .cloned()
                    .ok_or(Error::QueueChanged)
            })
            .await?;
        let registry = self.vertical_zone.published_registry.clone();
        let session = self.session.clone();
        let visits = self.vertical_zone_detail.visits.clone();
        let receipt = receipt.to_owned();
        let expected = snapshot.clone();
        let guard = Cancellation::new(move || {
            session
                .with_verified_profile(&authority.token, &authority.api, |_, _| {
                    authority.check_revision()?;
                    let registry = registry.lock().map_err(|_| Error::LockPoisoned)?;
                    let visits = visits.lock().map_err(|_| Error::LockPoisoned)?;
                    Ok(visits.iter().any(|visit| {
                        visit.receipt == receipt
                            && visit.read_sequence == expected.read_sequence
                            && visit
                                .snapshot
                                .as_ref()
                                .is_some_and(|actual| Arc::ptr_eq(actual, &expected))
                            && authority.matches(&visit.binding)
                            && authority.permits(registry.as_ref(), &visit.page_code)
                    }))
                })
                .unwrap_or(false)
        });
        Ok(ZoneBatchContext { snapshot, guard })
    }

    pub async fn vertical_zone_playlist_batch_page(
        &self,
        epoch: &str,
        receipt: &str,
        id: &str,
        page: PageRequest,
    ) -> Result<AccountData<TrackPage>> {
        let id = identifier(&json!(id))?;
        mapping::page(page.offset, page.limit)?;
        let revision = page.revision.as_deref().ok_or(Error::InvalidInput(
            "MGC playlist batch requires its detail revision",
        ))?;
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let context = self
            .zone_playlist_batch_context(&token, receipt, &id, revision)
            .await?;
        let loaded = context.snapshot.batch_loaded_prefix(&page)?;
        let mut changes = self.session.subscribe()?;
        let snapshot = &context.snapshot;
        let guard = &context.guard;
        let id = &id;
        let page = &page;
        let loaded = &loaded;
        let value = self
            .session
            .run_for(&token, |api, uid| async move {
                if guard.is_cancelled() {
                    return Err(Error::StaleResponse);
                }
                tokio::select! {
                    biased;
                    _ = changes.changed() => Err(Error::StaleResponse),
                    result = playlist_track_page(api, &uid, id, &snapshot.detail, page, Some(-1), Some(loaded)) => result,
                }
            })
            .await?;
        if context.guard.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
