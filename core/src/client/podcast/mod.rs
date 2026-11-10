pub(in crate::client) mod audiobook;
mod badges;
pub(in crate::client) mod browse_playback;
pub(super) use badges::Host as PodcastBadgeHost;
mod discovery;
mod dto;
mod home;
pub(in crate::client) mod liked_menu;
mod liked_page;
mod liked_playback;
mod program_menu;
pub(in crate::client) mod purchased;
pub use program_menu::{PodcastProgramTarget, PodcastRecentProgramMenu};
pub(super) mod profile;
pub(in crate::client) mod radio_playback;
mod rankings;
mod recent;
mod subscription_recommendations;
pub(super) use subscription_recommendations::Host as PodcastSubscriptionRecommendationHost;
pub use subscription_recommendations::{
    PodcastSubscriptionRecommendation, PodcastSubscriptionRecommendations,
};
pub(super) mod view;
use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    podcast::{PodcastAction, PodcastApi, PodcastLibrary, PodcastScope, PodcastService},
    protocol::api::identifier,
};
pub(super) use audiobook::Host as AudiobookSkipHost;
pub use audiobook::{AudiobookSkipAction, AudiobookSkipContext, AudiobookSkipView};
pub(super) use discovery::PodcastDiscoveryHost;
pub use discovery::{
    PodcastCatalogue, PodcastCatalogueCard, PodcastCatalogueQuery, PodcastCategory,
    PodcastSubcategory,
};
pub use dto::*;
pub use home::{PodcastHomeBlock, PodcastHomePage};
pub use liked_menu::PodcastLikedProgramTarget;
pub(super) use liked_page::PodcastLikedHost;
pub use liked_page::{PodcastLikedPage, PodcastLikedPageTarget, PodcastLikedQuery};
pub use rankings::{
    PodcastRankGroup, PodcastRankMenu, PodcastRankQuery, PodcastRankResource, PodcastRankRow,
    PodcastRankTab, PodcastRanks, PodcastTopLayout,
};
pub(super) use recent::PodcastRecentHost;
pub use recent::{PodcastRecentPage, PodcastRecentProgramTarget, PodcastRecentQuery};
use serde_json::{Value, json};

impl MusicClient {
    pub async fn podcast_search_membership(
        &self,
        epoch: &str,
    ) -> Result<super::dto::AccountData<PodcastSearchMembership>> {
        let token = self.podcast_token(epoch)?;
        let subscribed_ids = self
            .session
            .run_for(&token, |api, uid| async move {
                PodcastApi::new(api).search_subscribed_ids(&uid).await
            })
            .await
            .unwrap_or(None);
        let pending_ids = PodcastService::new(self.session.clone())?
            .operations_for(&token)?
            .into_iter()
            .filter(|operation| {
                operation.action == PodcastAction::Subscribe && operation.phase.pending()
            })
            .map(|operation| operation.id)
            .collect();
        Ok(super::dto::AccountData {
            epoch: epoch.into(),
            value: PodcastSearchMembership {
                subscribed_ids,
                pending_ids,
            },
        })
    }
    pub async fn podcast_state(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<super::dto::AccountData<PodcastState>> {
        let token = self.podcast_token(epoch)?;
        let library = PodcastLibrary::new(self.session.clone())?;
        let liked = if refresh {
            Some(library.read_for(&token, &PodcastScope::Liked, true).await?)
        } else {
            library.cached_for(&token, &PodcastScope::Liked)?
        };
        let operations = PodcastService::new(self.session.clone())?
            .operations_for(&token)?
            .into_iter()
            .filter(|v| v.phase.pending())
            .map(view::operation)
            .collect();
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(super::dto::AccountData {
            epoch: epoch.into(),
            value: PodcastState {
                liked_ids: liked.map(|s| s.voices.into_iter().map(|v| v.id).collect()),
                operations,
            },
        })
    }
    fn podcast_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    pub fn podcast_radio_order(
        &self,
        epoch: &str,
        query: PodcastRadioOrder,
    ) -> Result<super::dto::AccountData<Option<bool>>> {
        let token = self.podcast_token(epoch)?;
        let library = PodcastLibrary::new(self.session.clone())?;
        let value = match query {
            PodcastRadioOrder::Read { id } => library.radio_order_for(&token, &id)?,
            PodcastRadioOrder::Set {
                scope,
                revision,
                ascending,
            } => {
                library.set_radio_order_for(&token, &scope, &revision, ascending)?;
                Some(ascending)
            }
        };
        Ok(super::dto::AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn podcast_page(
        &self,
        epoch: &str,
        query: PodcastQuery,
        refresh: bool,
    ) -> Result<PodcastPage> {
        let token = self.podcast_token(epoch)?;
        super::mapping::page(query.offset, query.limit)?;
        if query.keyword.chars().count() > 200 {
            return Err(Error::InvalidInput("podcast search exceeds limit"));
        }
        let badges = matches!(query.scope, PodcastScope::Podcasts)
            .then(|| {
                self.podcast_badges
                    .begin(&self.session, &token, refresh)
                    .ok()
                    .flatten()
            })
            .flatten();
        let library = PodcastLibrary::new(self.session.clone())?;
        let extending = matches!(query.scope, PodcastScope::Purchased) && query.offset != 0;
        let snapshot = if extending {
            if refresh {
                return Err(Error::InvalidInput(
                    "purchased refresh must start at offset zero",
                ));
            }
            library
                .extend_purchased_for(
                    &token,
                    query.revision.as_deref().ok_or(Error::QueueChanged)?,
                    query.offset,
                )
                .await?
        } else {
            library.read_for(&token, &query.scope, refresh).await?
        };
        if !extending
            && query
                .revision
                .as_ref()
                .is_some_and(|r| r != &snapshot.revision)
        {
            return Err(Error::QueueChanged);
        }
        let liked = if matches!(query.scope, PodcastScope::Liked) {
            snapshot.clone()
        } else {
            library
                .read_for(&token, &PodcastScope::Liked, refresh)
                .await?
        };
        let liked_ids = liked
            .voices
            .iter()
            .map(|v| v.id.clone())
            .collect::<Vec<_>>();
        let resume = if let PodcastScope::Radio { id, .. } = &query.scope {
            Some(crate::podcast::resume::read_for(&self.session, &token, &json!(id)).await?)
        } else {
            None
        };
        let service = PodcastService::new(self.session.clone())?;
        let operations = service
            .operations_for(&token)?
            .into_iter()
            .filter(|v| v.phase.pending())
            .map(view::operation)
            .collect();
        let audiobook_skip_enabled = self.session.with_token(&token, |owner| {
            let PodcastScope::Radio { id, .. } = &query.scope else {
                return Ok(None);
            };
            let Some(radio) = snapshot
                .radios
                .first()
                .filter(|radio| radio.id == *id && audiobook::detail_category(&radio.raw))
            else {
                return Ok(None);
            };
            let store = self.session.persistent_store()?;
            use crate::podcast::AudiobookSkip;
            Ok(Some(
                AudiobookSkip::read(&store, owner, &radio.id)
                    .is_ok_and(|value| value != AudiobookSkip::Close),
            ))
        })?;
        let mut page = view::page(
            epoch,
            &snapshot,
            query,
            liked_ids,
            resume,
            operations,
            audiobook_skip_enabled,
        )?;
        self.podcast_badges
            .project(&self.session, &token, badges, &snapshot, &mut page)?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(page)
    }
    pub async fn podcast_command(
        &self,
        epoch: &str,
        command: PodcastCommand,
    ) -> Result<super::dto::AccountData<PodcastMutationView>> {
        let token = self.podcast_token(epoch)?;
        let service = PodcastService::new(self.session.clone())?;
        let operation = match command {
            PodcastCommand::CollectRecommendation {
                id,
                receipt,
                revision,
            } => {
                self.collect_subscription_recommendation(
                    &token,
                    &receipt,
                    &revision,
                    &identifier(&json!(id))?,
                )
                .await?
            }
            PodcastCommand::ChangeSearchSubscription { id, expected } => {
                service
                    .change_search_subscription_for(&token, &identifier(&json!(id))?, expected)
                    .await?
            }
            PodcastCommand::ChangeVinylSubscription { play_id, expected } => {
                self.change_vinyl_podcast_subscription(&token, &play_id, expected)
                    .await?
            }
            PodcastCommand::Change {
                action,
                id,
                desired,
            } => {
                service
                    .change_for(&token, action, &identifier(&json!(id))?, desired)
                    .await?
            }
            PodcastCommand::Reconcile { action, id } => {
                service
                    .reconcile_for(&token, action, &identifier(&json!(id))?)
                    .await?
            }
        };
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(super::dto::AccountData {
            epoch: epoch.into(),
            value: view::operation(operation),
        })
    }
    pub async fn change_fresh_vinyl_podcast_subscription(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        expected: bool,
    ) -> Result<super::dto::AccountData<PodcastMutationView>> {
        let operation = self
            .fresh_vinyl_podcast_operation(epoch, play_id, receipt, Some(expected))
            .await?;
        Ok(super::dto::AccountData {
            epoch: epoch.into(),
            value: view::operation(operation),
        })
    }

    pub async fn reconcile_fresh_vinyl_podcast_subscription(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
    ) -> Result<super::dto::AccountData<PodcastMutationView>> {
        let operation = self
            .fresh_vinyl_podcast_operation(epoch, play_id, receipt, None)
            .await?;
        Ok(super::dto::AccountData {
            epoch: epoch.into(),
            value: view::operation(operation),
        })
    }

    pub(super) async fn podcast_resources(
        &self,
        epoch: &str,
        scope: &PodcastScope,
        revision: &str,
        start_id: Option<&str>,
    ) -> Result<(Vec<Value>, usize)> {
        let token = self.podcast_token(epoch)?;
        if !matches!(scope, PodcastScope::Radio { .. } | PodcastScope::Liked) {
            return Err(Error::InvalidInput("podcast selection requires programs"));
        }
        // Re-read before replacing the queue so a stale displayed order cannot be
        // silently used after remote edits. It is independent of the visible page.
        let snapshot = PodcastLibrary::new(self.session.clone())?
            .read_for(&token, scope, true)
            .await?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        if *scope == PodcastScope::Liked {
            return self
                .liked_podcast_resources(&token, snapshot.voices, start_id)
                .await;
        }
        let index = if let Some(id) = start_id {
            snapshot
                .voices
                .iter()
                .position(|v| v.id == id)
                .ok_or(Error::QueueChanged)?
        } else {
            0
        };
        if snapshot.voices.is_empty() {
            return Err(Error::State("podcast queue is empty"));
        }
        let mut resources = Vec::with_capacity(snapshot.voices.len());
        for mut episode in snapshot.voices {
            if episode.song_id.is_none() {
                episode = self
                    .session
                    .run_for(&token, |api, _| async {
                        PodcastApi::new(api).episode(&json!(episode.id)).await
                    })
                    .await?;
            }
            let mut resource = episode.playback_resource()?;
            resource["song"] = episode.download_metadata()?;
            resource["song"]["voice_id"] = json!(episode.id);
            resource["level"] = json!("standard");
            resources.push(resource);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok((resources, index))
    }
}
