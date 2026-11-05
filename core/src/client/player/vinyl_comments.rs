//! Standard vinyl's read-only current-track comments. The renderer cannot name
//! a thread, promote an experiment group, or use the route for another track.
use super::{MusicClient, PlaybackCommand, PlayerResourceKind, identity};
use crate::{
    Error, Result,
    client::{
        comments::{CommentResource, CommentView, comment},
        dto::AccountData,
    },
    comments::{Comments, ResourceKind, vinyl_query},
    playback::controller::PlaybackSnapshot,
    protocol::api::identifier,
};
use serde::Serialize;

pub(super) const EXPERIMENT: &str = "PH-PC-optimizeComment";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylCommentPageView {
    pub latest: Vec<CommentView>,
    pub hot: Vec<CommentView>,
    pub more_hot: bool,
    pub next_offset: Option<usize>,
}

pub(super) fn song(snapshot: &PlaybackSnapshot, play_id: &str) -> Result<CommentResource> {
    if snapshot.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    let resource = &snapshot
        .selected
        .as_ref()
        .ok_or(Error::State("vinyl comments require current music"))?
        .resource;
    if identity::kind(resource) != PlayerResourceKind::Catalog {
        return Err(Error::State("vinyl comments require current catalog music"));
    }
    Ok(CommentResource {
        kind: ResourceKind::Track,
        id: identifier(&resource["id"])?,
        owner: None,
    })
}

impl MusicClient {
    pub async fn player_vinyl_comments_available(&self, epoch: &str) -> Result<bool> {
        Ok(self
            .verified_pc_experiment_groups(epoch)
            .await?
            .group(EXPERIMENT)
            == Some("t1"))
    }

    pub async fn player_vinyl_comments(
        &self,
        epoch: &str,
        play_id: &str,
        offset: usize,
    ) -> Result<AccountData<VinylCommentPageView>> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller().await?;
        let snapshot = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        let resource = song(&snapshot, play_id)?;
        let query = vinyl_query(offset);
        query.validate()?;
        let (revision, groups) = self.verified_pc_experiment_groups_for(&token, &api).await?;
        if groups.group(EXPERIMENT) != Some("t1") {
            return Err(Error::State("vinyl comments experiment is unavailable"));
        }
        let before = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        if song(&before, play_id)? != resource
            || api.verified_pc_experiment_revision()?.as_deref() != Some(revision.as_str())
        {
            return Err(Error::StaleResponse);
        }
        // M438 defaults beforeTime to zero on every append. The independent
        // comments page's cursor carries time and must not drive this lane.
        let page = Comments::new(self.session.clone())?
            .vinyl_page_for(&token, &resource, &query, &api, &revision)
            .await
            .map_err(|error| match error {
                Error::CredentialChanged => Error::StaleResponse,
                other => other,
            })?;
        let after = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.require_api(&token, &api)?;
        if song(&after, play_id)? != resource
            || api.verified_pc_experiment_revision()?.as_deref() != Some(revision.as_str())
        {
            return Err(Error::StaleResponse);
        }
        let next_offset = page.more.then_some(offset + page.comments.len());
        self.session.with_token(&token, |uid| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: VinylCommentPageView {
                    latest: page
                        .comments
                        .into_iter()
                        .map(|item| comment(item, uid))
                        .collect(),
                    hot: if offset == 0 {
                        page.hot_comments
                            .into_iter()
                            .take(5)
                            .map(|item| comment(item, uid))
                            .collect()
                    } else {
                        Vec::new()
                    },
                    more_hot: offset == 0 && page.more_hot,
                    next_offset,
                },
            })
        })
    }
}
