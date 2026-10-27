use super::{AccountData, CommentView, MusicClient, comment};
use crate::{
    Result,
    comments::{CommentResource, CommentWrite, CommentWriteCommand, CommentWritePhase, Comments},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentWriteView {
    pub id: String,
    pub resource: CommentResource,
    pub command: CommentWriteCommand,
    pub phase: CommentWritePhase,
    pub created_at: u64,
    pub acknowledged: bool,
    pub receipt_id: Option<String>,
    pub target: Option<CommentView>,
    pub observed: Option<CommentView>,
    pub observed_absent: bool,
    pub service_code: Option<i64>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommentWriteAction {
    Submit,
    Reconcile,
    Abandon,
}
fn view(record: CommentWrite, owner: &str) -> CommentWriteView {
    CommentWriteView {
        id: record.id,
        resource: record.resource,
        command: record.command,
        phase: record.phase,
        created_at: record.created_at,
        acknowledged: record.acknowledged,
        receipt_id: record.receipt_id,
        target: record.before.map(|v| comment(v, owner)),
        observed: record.observed.map(|v| comment(v, owner)),
        observed_absent: record.observed_absent,
        service_code: record.service_code,
    }
}
impl MusicClient {
    pub async fn comments_prepare_write(
        &self,
        epoch: &str,
        resource: CommentResource,
        command: CommentWriteCommand,
    ) -> Result<AccountData<CommentWriteView>> {
        let token = self.comment_token(epoch)?;
        let record = Comments::new(self.session.clone())?
            .prepare_write_for(&token, &resource, command)
            .await?;
        self.session.with_token(&token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: view(record, owner),
            })
        })
    }
    pub fn comments_write_operations(
        &self,
        epoch: &str,
        resource: CommentResource,
    ) -> Result<AccountData<Vec<CommentWriteView>>> {
        let token = self.comment_token(epoch)?;
        let records =
            Comments::new(self.session.clone())?.write_operations_for(&token, &resource)?;
        self.session.with_token(&token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: records.into_iter().map(|v| view(v, owner)).collect(),
            })
        })
    }
    pub async fn comments_write_command(
        &self,
        epoch: &str,
        id: &str,
        command: CommentWriteAction,
    ) -> Result<AccountData<CommentWriteView>> {
        let token = self.comment_token(epoch)?;
        let service = Comments::new(self.session.clone())?;
        let record = match command {
            CommentWriteAction::Submit => service.submit_write_for(&token, id).await?,
            CommentWriteAction::Reconcile => service.reconcile_write_for(&token, id).await?,
            CommentWriteAction::Abandon => service.abandon_write_for(&token, id)?,
        };
        self.session.with_token(&token, |owner| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: view(record, owner),
            })
        })
    }
}
