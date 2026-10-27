use super::*;
use crate::{
    Error, Result,
    account::{SessionToken, resources::MutationPhase},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(super) const PAGES: &str = "comment-pages-v1";
pub(super) const REVISIONS: &str = "comment-page-revisions-v1";
#[derive(Serialize, Deserialize)]
pub(super) struct CachedPage {
    pub resource: CommentResource,
    pub query: CommentQuery,
    pub page: CommentPage,
}
impl Comments {
    pub(super) fn revision(
        &self,
        token: &SessionToken,
        resource: &CommentResource,
    ) -> Result<Option<Value>> {
        let thread = resource.thread()?;
        self.session
            .with_token(token, |owner| self.store.get(owner, REVISIONS, &thread))
    }
    pub(super) fn cache_page(
        &self,
        token: &SessionToken,
        key: &str,
        revision: Option<Value>,
        value: CachedPage,
    ) -> Result<CommentPage> {
        let thread = value.resource.thread()?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                if tx.get(owner, REVISIONS, &thread)? != revision {
                    return Err(Error::QueueChanged);
                }
                tx.put(owner, PAGES, key, &serde_json::to_value(&value)?)?;
                Ok(value.page)
            })
        })
    }
    pub(super) fn save_like_transition(
        &self,
        token: &SessionToken,
        key: &str,
        record: &CommentLike,
    ) -> Result<()> {
        let thread = record.resource.thread()?;
        self.session.with_token(token, |owner| {
            self.store.transaction(|tx| {
                tx.put(
                    owner,
                    super::likes::JOURNAL,
                    key,
                    &serde_json::to_value(record)?,
                )?;
                tx.put(
                    owner,
                    REVISIONS,
                    &thread,
                    &json!(uuid::Uuid::new_v4().simple().to_string()),
                )?;
                if matches!(
                    record.phase,
                    MutationPhase::Confirmed | MutationPhase::Unchanged
                ) {
                    let observed = record
                        .observed
                        .as_ref()
                        .ok_or(Error::State("confirmed comment observation missing"))?;
                    for row in tx.rows(PAGES)? {
                        if row.owner != owner {
                            continue;
                        }
                        // Legacy caches predate the resource envelope. They cannot
                        // safely be patched by ID alone, so evict only those caches.
                        if row.value.get("resource").is_none() {
                            tx.delete(owner, PAGES, &row.key)?;
                            continue;
                        }
                        let mut cache: CachedPage = serde_json::from_value(row.value)?;
                        if cache.resource != record.resource {
                            continue;
                        }
                        for comment in cache
                            .page
                            .comments
                            .iter_mut()
                            .chain(&mut cache.page.hot_comments)
                            .chain(&mut cache.page.top_comments)
                            .chain(cache.page.current_comment.iter_mut())
                        {
                            if comment.id == observed.id {
                                *comment = observed.clone();
                            }
                        }
                        tx.put(owner, PAGES, &row.key, &serde_json::to_value(cache)?)?;
                    }
                }
                Ok(())
            })
        })
    }
}
