mod dto;
pub(super) mod view;
use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    podcast::{PodcastApi, PodcastLibrary, PodcastScope, PodcastService},
    protocol::api::identifier,
};
pub use dto::*;
use serde_json::{Value, json};

impl MusicClient {
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
        let library = PodcastLibrary::new(self.session.clone())?;
        let snapshot = library.read_for(&token, &query.scope, refresh).await?;
        if query
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
            Some(
                self.session
                    .run_for(&token, |api, _| async {
                        PodcastApi::new(api).resume_enabled(&json!(id)).await
                    })
                    .await?,
            )
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
        self.session.with_token(&token, |_| Ok(()))?;
        view::page(epoch, snapshot, query, liked_ids, resume, operations)
    }
    pub async fn podcast_command(
        &self,
        epoch: &str,
        command: PodcastCommand,
    ) -> Result<super::dto::AccountData<PodcastMutationView>> {
        let token = self.podcast_token(epoch)?;
        let service = PodcastService::new(self.session.clone())?;
        let operation = match command {
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
