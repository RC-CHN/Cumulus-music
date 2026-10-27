use super::*;
use crate::{
    Result,
    account::{AccountSession, SessionToken},
    storage::StateStore,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
pub struct Comments {
    pub(super) session: Arc<AccountSession>,
    pub(super) store: Arc<StateStore>,
    pub(super) settle: std::time::Duration,
}
impl Comments {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
            settle: std::time::Duration::from_millis(500),
        })
    }
    /// Dependency-injected readback delay for native tests, never a write retry.
    pub fn with_settle_delay(mut self, delay: std::time::Duration) -> Self {
        self.settle = delay;
        self
    }
    fn page_key(resource: &CommentResource, query: &CommentQuery) -> Result<String> {
        resource.thread()?;
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
            resource, query,
        ))?)))
    }
    pub async fn page_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        query: &CommentQuery,
    ) -> Result<CommentPage> {
        let key = Self::page_key(resource, query)?;
        let revision = self.revision(token, resource)?;
        let page = self
            .session
            .run_for(token, |api, _| async move {
                CommentApi::new(api).page(resource, query).await
            })
            .await?;
        self.cache_page(
            token,
            &key,
            revision,
            cache::CachedPage {
                resource: resource.clone(),
                query: query.clone(),
                page,
            },
        )
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        query: &CommentQuery,
    ) -> Result<Option<CommentPage>> {
        let key = Self::page_key(resource, query)?;
        self.session.with_token(token, |owner| {
            self.store
                .get(owner, cache::PAGES, &key)?
                .map(|value| -> Result<CommentPage> {
                    if value.get("resource").is_some() {
                        let cached: cache::CachedPage = serde_json::from_value(value)?;
                        if cached.resource != *resource || cached.query != *query {
                            return Err(crate::Error::State("comment cache identity mismatch"));
                        }
                        Ok(cached.page)
                    } else {
                        Ok(serde_json::from_value(value)?)
                    }
                })
                .transpose()
        })
    }
    pub async fn target_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
    ) -> Result<Comment> {
        self.session
            .run_for(token, |api, _| async move {
                CommentApi::new(api).target(resource, id).await
            })
            .await
    }
    pub async fn lookup_for(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
        id: &str,
    ) -> Result<Option<Comment>> {
        self.session
            .run_for(token, |api, _| async move {
                CommentApi::new(api).lookup(resource, id).await
            })
            .await
    }
}
