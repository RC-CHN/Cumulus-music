//! Keep the correction reminder's channel on the native side of the bridge.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    catalog::search::results::{
        CorrectionContext, ResultContext, SearchCategory, SearchResultQuery,
    },
    protocol::api::Api,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use uuid::Uuid;

const LIMIT: usize = 64;
const LIFETIME: Duration = Duration::from_secs(20 * 60);

#[derive(Default)]
pub(super) struct CorrectionChannels(Mutex<VecDeque<Entry>>);

struct Entry {
    receipt: Uuid,
    owner: SessionToken,
    api: Weak<dyn Api>,
    keyword: String,
    created: Instant,
    context: CorrectionContext,
}

impl Entry {
    fn is_current(&self, owner: &SessionToken, api: &Arc<dyn Api>, revision: &str) -> bool {
        self.owner == *owner
            && self.api.ptr_eq(&Arc::downgrade(api))
            && self.context.revision == revision
            && self.created.elapsed() < LIFETIME
    }
}

impl CorrectionChannels {
    pub(super) fn capture(
        &self,
        session: &AccountSession,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        before: Option<&str>,
        keyword: &str,
        channel: Option<String>,
    ) -> Result<Option<String>> {
        let (Some(channel), Some(revision)) = (channel, before) else {
            return Ok(None);
        };
        session.require_api(owner, api)?;
        if api
            .search_default_context_revision()
            .ok()
            .flatten()
            .as_deref()
            != Some(revision)
        {
            return Ok(None);
        }
        let mut entries = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        entries.retain(|entry| entry.is_current(owner, api, revision));
        let receipt = Uuid::new_v4();
        entries.push_back(Entry {
            receipt,
            owner: owner.clone(),
            api: Arc::downgrade(api),
            keyword: keyword.into(),
            created: Instant::now(),
            context: CorrectionContext {
                channel,
                revision: revision.into(),
            },
        });
        while entries.len() > LIMIT {
            entries.pop_front();
        }
        Ok(Some(receipt.simple().to_string()))
    }

    pub(super) fn resolve(
        &self,
        session: &AccountSession,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        query: &SearchResultQuery,
        receipt: Option<&str>,
    ) -> Result<Option<ResultContext>> {
        if query.correct
            || query.channel.is_some()
            || !matches!(
                query.category,
                SearchCategory::All | SearchCategory::Songs | SearchCategory::Mvs
            )
        {
            return Ok(None);
        }
        let Some(receipt) = receipt
            .filter(|id| id.len() == 32)
            .and_then(|id| Uuid::parse_str(id).ok())
        else {
            return Ok(None);
        };
        session.require_api(owner, api)?;
        let Some(revision) = api.search_default_context_revision().ok().flatten() else {
            return Ok(None);
        };
        let mut entries = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        entries.retain(|entry| entry.is_current(owner, api, &revision));
        Ok(entries
            .iter()
            .find(|entry| entry.receipt == receipt && entry.keyword == query.keyword.trim())
            .map(|entry| ResultContext::Correction(entry.context.clone())))
    }
}
