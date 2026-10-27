use super::*;
use crate::{
    account::resources::{AccountResources, ResourceKind},
    client::dto::AccountData,
    mv::{MvDetail, MvService},
    protocol::api::identifier,
};
use serde_json::json;
use sha2::{Digest, Sha256};
impl MusicClient {
    pub async fn mv_page(&self, epoch: &str, query: MvQuery, refresh: bool) -> Result<MvPage> {
        let token = self.mv.token(epoch)?;
        if query.limit == 0
            || query.limit > 100
            || query.offset > 200000
            || query.keyword.chars().count() > 200
        {
            return Err(Error::InvalidInput("invalid MV page request"));
        }
        let resources = AccountResources::new(self.session.clone())?;
        let cached = self.session.with_token(&token, |uid| {
            self.session
                .persistent_store()?
                .get(uid, "resources", "mvs")
        })?;
        let value = match cached {
            Some(value) if !refresh => value,
            _ => resources.refresh_for(&token, ResourceKind::Mvs).await?,
        };
        let revision = hex::encode(Sha256::digest(serde_json::to_vec(&value)?));
        if query.revision.as_ref().is_some_and(|r| r != &revision) {
            return Err(Error::QueueChanged);
        }
        let rows: Vec<MvDetail> = serde_json::from_value(value)?;
        let keyword = query.keyword.trim().to_lowercase();
        let cards = rows
            .iter()
            .map(view::card)
            .filter(|c| {
                keyword.is_empty()
                    || format!("{} {}", c.title, c.artists.join(" "))
                        .to_lowercase()
                        .contains(&keyword)
            })
            .collect::<Vec<_>>();
        let total = cards.len();
        let operations = resources
            .operations_for(&token)?
            .into_values()
            .filter(|o| o.kind == ResourceKind::Mvs && o.phase.pending())
            .map(view::operation)
            .collect();
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(MvPage {
            epoch: epoch.into(),
            revision,
            cards: cards
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect(),
            total,
            offset: query.offset,
            operations,
        })
    }
    pub async fn mv_detail(&self, epoch: &str, id: &str) -> Result<AccountData<MvView>> {
        let token = self.mv.token(epoch)?;
        let id = identifier(&json!(id))?;
        let service = MvService::new(self.session.clone())?;
        let result = service.download_options_for(&token, &json!(id)).await;
        let (detail, cached) = match result {
            Ok(d) => (d, false),
            Err(e) => match service.cached_for(&token, &json!(id))? {
                Some(d) => (d, true),
                None => return Err(e),
            },
        };
        let operation = service.like_operation_for(&token, &id)?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: view::detail(detail, cached, operation)?,
            })
        })
    }
    pub async fn mv_collect(
        &self,
        epoch: &str,
        id: &str,
        desired: bool,
    ) -> Result<AccountData<MvMutationView>> {
        let token = self.mv.token(epoch)?;
        let record = AccountResources::new(self.session.clone())?
            .set_membership_for(&token, ResourceKind::Mvs, &json!(id), desired)
            .await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: view::operation(record),
        })
    }
    pub async fn mv_reconcile(&self, epoch: &str, id: &str) -> Result<AccountData<MvMutationView>> {
        let token = self.mv.token(epoch)?;
        let record = AccountResources::new(self.session.clone())?
            .reconcile_expected(&token, ResourceKind::Mvs, id)
            .await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: view::operation(record),
        })
    }
}
