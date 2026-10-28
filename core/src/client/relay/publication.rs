use super::super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    relay::publisher::{
        PublicationIntent, PublicationPhase, PublicationReceipt, RecoveryReceipt, RelayPublisher,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPublicationView {
    pub pending: usize,
    pub blocked: Option<RelayPublicationReceipt>,
    pub recovering: Option<RelayPublicationReceipt>,
    pub latest: Option<RelayPublicationReceipt>,
    pub requested: Option<RelayPublicationReceipt>,
    pub can_resynchronize: bool,
    pub sending: bool,
    pub record_failed: bool,
    pub preflight_failed: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayPublicationReceipt {
    pub id: String,
    pub phase: &'static str,
    pub recovery_of: Option<String>,
    pub service_code: Option<i64>,
    pub created_at: u64,
}
impl From<PublicationIntent> for RelayPublicationReceipt {
    fn from(row: PublicationIntent) -> Self {
        PublicationReceipt::from(row).into()
    }
}
impl From<PublicationReceipt> for RelayPublicationReceipt {
    fn from(row: PublicationReceipt) -> Self {
        Self {
            id: row.id,
            phase: match row.phase {
                PublicationPhase::Prepared => "prepared",
                PublicationPhase::Sending => "sending",
                PublicationPhase::Confirmed => "confirmed",
                PublicationPhase::Rejected => "rejected",
                PublicationPhase::Uncertain => "uncertain",
                PublicationPhase::Skipped => "skipped",
                PublicationPhase::Superseded => "superseded",
            },
            recovery_of: row.recovery_of,
            service_code: row.service_code,
            created_at: row.created_at,
        }
    }
}
impl From<RecoveryReceipt> for RelayPublicationReceipt {
    fn from(row: RecoveryReceipt) -> Self {
        match row {
            RecoveryReceipt::Recorded(row) => row.into(),
            RecoveryReceipt::Cancelled {
                id,
                blocked,
                created_at,
            } => Self {
                id,
                phase: "cancelled",
                recovery_of: Some(blocked),
                service_code: None,
                created_at,
            },
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelayPublicationCommand {
    Resynchronize { id: String, blocked_id: String },
    CancelUnseen { id: String, blocked_id: String },
}
impl MusicClient {
    /// Local white-list projection. No queue body, relay session/device identity
    /// or signed resource is sent to WebView. Reading never starts audio/HTTP.
    pub fn relay_publication_view(
        &self,
        epoch: &str,
        requested: Option<&str>,
    ) -> Result<AccountData<RelayPublicationView>> {
        let token = self.player_token(epoch)?;
        let publisher = RelayPublisher::new(self.session.clone())?;
        let status = publisher.inspect_for(&token)?;
        let runtime = self
            .player
            .controller
            .get()
            .and_then(|p| p.relay_runtime())
            .map(|r| r.status())
            .transpose()?
            .filter(|h| h.account == token);
        let record_failed = runtime.as_ref().is_some_and(|h| h.record_failed);
        let value = RelayPublicationView {
            pending: status.pending,
            can_resynchronize: status.can_resynchronize && !record_failed,
            blocked: status.blocked.map(Into::into),
            recovering: status.recovering.map(Into::into),
            latest: status.latest.map(Into::into),
            requested: requested
                .map(|id| publisher.recovery_receipt_for(&token, id))
                .transpose()?
                .flatten()
                .map(Into::into),
            sending: runtime.as_ref().is_some_and(|h| h.sending),
            record_failed,
            preflight_failed: runtime.as_ref().is_some_and(|h| h.preflight_failed),
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
    pub async fn relay_publication_command(
        &self,
        epoch: &str,
        command: RelayPublicationCommand,
    ) -> Result<AccountData<RelayPublicationView>> {
        let token = self.player_token(epoch)?;
        let publisher = RelayPublisher::new(self.session.clone())?;
        let id = match command {
            RelayPublicationCommand::CancelUnseen { id, blocked_id } => {
                publisher.cancel_unseen_for(&token, &id, &blocked_id)?;
                id
            }
            RelayPublicationCommand::Resynchronize { id, blocked_id } => {
                let runtime = self.player.controller.get().and_then(|p| p.relay_runtime());
                if runtime
                    .map(|r| r.status())
                    .transpose()?
                    .is_some_and(|h| h.account == token && h.record_failed)
                {
                    return Err(Error::State("relay local recording requires repair"));
                }
                let row = publisher
                    .resynchronize_for(&token, &id, &blocked_id)
                    .await?;
                if row.phase == PublicationPhase::Prepared {
                    if let Some(runtime) = runtime {
                        runtime.wake();
                    } else {
                        publisher.publish_next_for(&token).await?;
                    }
                }
                id
            }
        };
        self.relay_publication_view(epoch, Some(&id))
    }
}
