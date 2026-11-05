//! Pair a displayed official default word with its native-only query context.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    catalog::search::{DefaultWord, SearchWord, results::DefaultContext},
    protocol::api::Api,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
};
use uuid::Uuid;

const MAX_CONTEXTS: usize = 80;

#[derive(Default)]
pub(super) struct DefaultContexts(Mutex<VecDeque<Entry>>);

struct Entry {
    receipt: Uuid,
    owner: SessionToken,
    api: Weak<dyn Api>,
    keyword: String,
    context: DefaultContext,
}

impl Entry {
    fn belongs_to(&self, owner: &SessionToken, api: &Arc<dyn Api>, revision: &str) -> bool {
        self.owner == *owner
            && self.context.revision == revision
            && self.api.ptr_eq(&Arc::downgrade(api))
    }
}

impl DefaultContexts {
    pub(super) fn publish(
        &self,
        session: &AccountSession,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        before: Option<&str>,
        rows: Vec<DefaultWord>,
    ) -> Result<Vec<SearchWord>> {
        session.require_api(owner, api)?;
        let revision = api.search_default_context_revision().ok().flatten();
        let mut entries = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        entries.retain(|entry| {
            revision
                .as_ref()
                .is_some_and(|rev| entry.belongs_to(owner, api, rev))
        });
        let mut words = Vec::with_capacity(rows.len());
        for DefaultWord { mut word, context } in rows {
            if let (Some(value), Some(revision)) = (
                context,
                revision.as_ref().filter(|rev| Some(rev.as_str()) == before),
            ) {
                let receipt = Uuid::new_v4();
                entries.push_back(Entry {
                    receipt,
                    owner: owner.clone(),
                    api: Arc::downgrade(api),
                    keyword: word.keyword.clone(),
                    context: DefaultContext {
                        value,
                        revision: revision.clone(),
                    },
                });
                word.default_receipt = Some(receipt.simple().to_string());
            }
            words.push(word);
        }
        while entries.len() > MAX_CONTEXTS {
            entries.pop_front();
        }
        Ok(words)
    }

    pub(super) fn resolve(
        &self,
        session: &AccountSession,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        keyword: &str,
        receipt: Option<&str>,
    ) -> Result<Option<DefaultContext>> {
        let Some(receipt) = receipt
            .filter(|id| id.len() == 32)
            .and_then(|id| Uuid::parse_str(id).ok())
        else {
            return Ok(None);
        };
        session.require_api(owner, api)?;
        let revision = api.search_default_context_revision().ok().flatten();
        let mut entries = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        entries.retain(|entry| {
            revision
                .as_ref()
                .is_some_and(|rev| entry.belongs_to(owner, api, rev))
        });
        Ok(entries
            .iter()
            .find(|entry| entry.receipt == receipt && entry.keyword == keyword)
            .map(|entry| entry.context.clone()))
    }
}
