//! Standard vinyl's current-thread comment likes use the independent page's
//! durable journal, with the same native dispatch fence as the composer.
use super::MusicClient;
use crate::{
    Error, Result,
    client::{
        comments::{CommentOperationView, operation},
        dto::AccountData,
    },
    comments::{Comments, VinylWriteAuthority},
    protocol::api::VinylCommentRead,
};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VinylCommentLikeAction {
    Set { desired: bool },
    Reconcile,
}

impl MusicClient {
    pub async fn player_vinyl_comment_like_operations(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<AccountData<Vec<CommentOperationView>>> {
        let target = self.vinyl_write_target(epoch, play_id).await?;
        let records =
            Comments::new(self.session.clone())?.operations_for(&target.token, &target.resource)?;
        target.check_published_play(play_id)?;
        self.session.with_token(&target.token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: records
                    .into_iter()
                    .map(|row| operation(row, owner))
                    .collect(),
            })
        })
    }

    pub async fn player_vinyl_comment_like(
        &self,
        epoch: &str,
        play_id: &str,
        id: &str,
        action: VinylCommentLikeAction,
    ) -> Result<AccountData<CommentOperationView>> {
        let target = self.vinyl_write_target(epoch, play_id).await?;
        // Reconciliation accepts only the same canonical current-song target.
        VinylCommentRead::target(&target.resource.id, id)?;
        let service = Comments::new(self.session.clone())?;
        let result = match action {
            VinylCommentLikeAction::Set { desired } => {
                let used = AtomicBool::new(false);
                let permission = || {
                    target.permit_live(self, play_id, || {
                        if used.swap(true, Ordering::Relaxed) {
                            return Err(Error::StaleResponse);
                        }
                        Ok(())
                    })
                };
                service
                    .like_for_vinyl(
                        &target.token,
                        &target.resource,
                        id,
                        desired,
                        VinylWriteAuthority {
                            api: &target.api,
                            revision: &target.revision,
                            permission: &permission,
                        },
                    )
                    .await?
            }
            VinylCommentLikeAction::Reconcile => {
                service
                    .reconcile_for_vinyl(
                        &target.token,
                        &target.resource,
                        id,
                        &target.api,
                        &target.revision,
                    )
                    .await?
            }
        };
        target.check(self, play_id).await?;
        self.session.with_token(&target.token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: operation(result, owner),
            })
        })
    }
}
