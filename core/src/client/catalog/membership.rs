use super::super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::resources::{AccountResources, MutationPhase, MutationRecord, ResourceKind},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogResource {
    Artist { id: String },
    Album { id: String },
}
impl CatalogResource {
    fn parts(&self) -> (ResourceKind, &str) {
        match self {
            Self::Artist { id } => (ResourceKind::Artists, id),
            Self::Album { id } => (ResourceKind::Albums, id),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogMembershipAction {
    Inspect,
    Set { desired: bool },
    Reconcile,
}
#[derive(Serialize)]
pub struct CatalogMembershipOperation {
    pub phase: MutationPhase,
    pub desired: bool,
    pub observed: Option<bool>,
}
#[derive(Serialize)]
pub struct CatalogMembershipView {
    pub resource: CatalogResource,
    pub operation: Option<CatalogMembershipOperation>,
}
fn operation(record: MutationRecord) -> Result<CatalogMembershipOperation> {
    Ok(CatalogMembershipOperation {
        phase: record.phase,
        desired: record
            .desired
            .as_bool()
            .ok_or(Error::State("invalid catalog membership intent"))?,
        observed: if record.phase == MutationPhase::Unchanged {
            record.before.as_bool()
        } else {
            record.observed.and_then(|v| v.as_bool())
        },
    })
}
impl MusicClient {
    pub async fn catalog_membership(
        &self,
        epoch: &str,
        resource: CatalogResource,
        action: CatalogMembershipAction,
    ) -> Result<AccountData<CatalogMembershipView>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let (kind, id) = resource.parts();
        identifier(&json!(id))?;
        let service = AccountResources::new(self.session.clone())?;
        let record = match action {
            CatalogMembershipAction::Inspect => service
                .operations_for(&token)?
                .into_values()
                .find(|r| r.kind == kind && r.key == id),
            CatalogMembershipAction::Set { desired } => Some(
                service
                    .set_membership_for(&token, kind, &json!(id), desired)
                    .await?,
            ),
            CatalogMembershipAction::Reconcile => {
                Some(service.reconcile_expected(&token, kind, id).await?)
            }
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: CatalogMembershipView {
                    resource,
                    operation: record.map(operation).transpose()?,
                },
            })
        })
    }
}
