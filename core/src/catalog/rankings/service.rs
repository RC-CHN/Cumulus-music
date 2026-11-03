use super::*;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
const CACHE: &str = "ranking-pages-v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedRanking {
    pub query: RankingQuery,
    pub fetched_at: u64,
    pub page: RankingPage,
}
const DEFAULT_HOURS: f64 = 3.0;
const MS_PER_HOUR: f64 = 3_600_000.0;
const MAX_EXACT_MS: f64 = 9_007_199_254_740_991.0;

impl SavedRanking {
    /// The official fallback when no verified custom PC value is available.
    pub fn fresh_at(&self, now: u64) -> bool {
        self.fresh_at_with_choice_hours(now, None)
    }

    /// Official M400 -> M460 -> M32.n: hours minus the smaller of 5% or
    /// three seconds. Reject nonsensical/non-exact-range values and backwards clocks.
    pub(crate) fn fresh_at_with_choice_hours(&self, now: u64, hours: Option<f64>) -> bool {
        let ttl = hours
            .map(|hours| hours * MS_PER_HOUR)
            .filter(|ms| *ms > 0.0 && *ms <= MAX_EXACT_MS)
            .unwrap_or(DEFAULT_HOURS * MS_PER_HOUR);
        matches!(self.query, RankingQuery::Home)
            && now
                .checked_sub(self.fetched_at)
                .is_some_and(|age| age as f64 <= (0.95 * ttl).max(ttl - 3_000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_ttl_uses_inclusive_official_cutoff_and_bounds() {
        let mut saved = SavedRanking {
            query: RankingQuery::Home,
            fetched_at: 1_000,
            page: RankingPage::Home { groups: vec![] },
        };
        for (hours, last_fresh_ms) in [
            (None, 10_797_000),
            (Some(2.0), 7_197_000),
            (Some(0.01), 34_200),
            (Some(0.0), 10_797_000),
            (Some(-1.0), 10_797_000),
            (Some(f64::INFINITY), 10_797_000),
            (Some(1e30), 10_797_000),
        ] {
            assert!(saved.fresh_at_with_choice_hours(saved.fetched_at + last_fresh_ms, hours));
            assert!(!saved.fresh_at_with_choice_hours(saved.fetched_at + last_fresh_ms + 1, hours));
            assert!(!saved.fresh_at_with_choice_hours(saved.fetched_at - 1, hours));
        }
        saved.query = RankingQuery::SongSales;
        assert!(!saved.fresh_at_with_choice_hours(saved.fetched_at, Some(2.0)));
    }
}
pub struct Rankings {
    session: Arc<AccountSession>,
    store: Arc<StateStore>,
}
impl Rankings {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        Ok(Self {
            store: session.persistent_store()?,
            session,
        })
    }
    fn key(query: &RankingQuery) -> Result<String> {
        query.validate()?;
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(query)?)))
    }
    pub fn cached_for(
        &self,
        token: &SessionToken,
        query: &RankingQuery,
    ) -> Result<Option<SavedRanking>> {
        let key = Self::key(query)?;
        self.session.with_token(token, |uid| {
            self.store
                .get(uid, CACHE, &key)?
                .map(|v| {
                    let saved: SavedRanking = serde_json::from_value(v)?;
                    if saved.query != *query {
                        return Err(Error::State("ranking cache identity mismatch"));
                    }
                    Ok(saved)
                })
                .transpose()
        })
    }
    pub async fn page_for(
        &self,
        token: &SessionToken,
        query: &RankingQuery,
    ) -> Result<SavedRanking> {
        let key = Self::key(query)?;
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let _lock =
            OperationLock::acquire(self.store.directory(), CACHE, &format!("{owner}:{key}"))?;
        let page = self
            .session
            .run_for(token, |api, _| async move {
                RankingsApi::new(api).page(query).await
            })
            .await?;
        let saved = SavedRanking {
            query: query.clone(),
            fetched_at: (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).max(0)
                as u64,
            page,
        };
        self.session.with_token(token, |uid| {
            self.store
                .put(uid, CACHE, &key, &serde_json::to_value(&saved)?)
        })?;
        Ok(saved)
    }
}
