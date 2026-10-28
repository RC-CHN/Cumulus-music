mod cards;
mod dto;
mod membership;
mod project;
mod selection;
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    catalog::details::{CatalogDetails, CatalogQuery},
};
pub use dto::*;
pub use membership::{
    CatalogMembershipAction, CatalogMembershipOperation, CatalogMembershipView, CatalogResource,
};
pub use selection::CatalogSelectionPage;

impl MusicClient {
    pub async fn catalog_page(
        &self,
        epoch: &str,
        query: CatalogQuery,
        refresh: bool,
    ) -> Result<AccountData<Option<CatalogView>>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let service = CatalogDetails::new(self.session.clone())?;
        let saved = if refresh {
            Some(service.page_for(&token, &query).await?)
        } else {
            service.cached_for(&token, &query)?
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: saved.map(|v| project::page(v, !refresh)).transpose()?,
            })
        })
    }
}
