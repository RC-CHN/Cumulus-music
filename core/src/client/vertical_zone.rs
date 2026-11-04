//! Authenticated vertical-zone catalogue and the original bounded tab cache.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::{AccountApi, SessionToken},
    catalog::vertical_zone::{VerticalZonePage, ZoneEntry, ZoneLayout, read, validate_page_code},
    protocol::api::Api,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
pub(super) struct ZoneHost(Mutex<ZoneCache>);

#[derive(Default)]
struct ZoneCache {
    owner: Option<(SessionToken, Weak<dyn Api>)>,
    sequence: u64,
    tabs: VecDeque<CacheItem>,
}

struct CacheItem {
    page_code: String,
    recommendation_enabled: bool,
    page: VerticalZonePage,
}

impl ZoneHost {
    fn enter(&self, token: &SessionToken, api: &Arc<dyn Api>) -> Result<u64> {
        let mut cache = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !cache.owner.as_ref().is_some_and(|(owner, native)| {
            owner == token
                && native
                    .upgrade()
                    .is_some_and(|native| Arc::ptr_eq(&native, api))
        }) {
            cache.owner = Some((token.clone(), Arc::downgrade(api)));
            cache.tabs.clear();
        }
        cache.sequence = cache
            .sequence
            .checked_add(1)
            .ok_or(Error::State("vertical zone sequence exhausted"))?;
        Ok(cache.sequence)
    }

    fn load(
        &self,
        sequence: u64,
        page_code: &str,
        entry: &ZoneEntry<'_>,
        enabled: bool,
        assigned: bool,
    ) -> Result<Option<VerticalZonePage>> {
        let mut cache = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if cache.sequence != sequence {
            return Err(Error::StaleResponse);
        }
        if !assigned {
            cache.tabs.clear();
            return Ok(None);
        }
        let Some(index) = cache.tabs.iter().position(|item| {
            if item.page_code != page_code
                || item.recommendation_enabled != enabled
                || item.page.tabs.is_empty()
            {
                return false;
            }
            item.page
                .active_tab
                .as_ref()
                .is_some_and(|tab| match entry {
                    ZoneEntry::Tab("") | ZoneEntry::SearchTabName("") => tab.key == "default",
                    ZoneEntry::Tab(key) => tab.key == *key,
                    ZoneEntry::SearchTabName(name) => tab.key == "default" && tab.name == *name,
                })
        }) else {
            return Ok(None);
        };
        let item = cache.tabs.remove(index).expect("cached tab");
        let page = item.page.clone();
        cache.tabs.push_back(item);
        Ok(Some(page))
    }

    fn finish(
        &self,
        sequence: u64,
        page_code: &str,
        enabled: bool,
        page: &VerticalZonePage,
    ) -> Result<()> {
        let mut cache = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if cache.sequence != sequence {
            return Err(Error::StaleResponse);
        }
        if page.layout == ZoneLayout::Tabs
            && let Some(tab) = &page.active_tab
        {
            cache.tabs.retain(|item| {
                item.page_code != page_code
                    || item
                        .page
                        .active_tab
                        .as_ref()
                        .is_none_or(|old| old.key != tab.key)
            });
            cache.tabs.push_back(CacheItem {
                page_code: page_code.into(),
                recommendation_enabled: enabled,
                page: page.clone(),
            });
            while cache.tabs.len() > 8 {
                cache.tabs.pop_front();
            }
        }
        Ok(())
    }
}

impl MusicClient {
    /// Called by trusted native navigation. The frontend cannot supply the
    /// experiment assignment, privacy bit or an upstream route.
    pub async fn vertical_zone(
        &self,
        epoch: &str,
        page_code: &str,
        entry: ZoneEntry<'_>,
    ) -> Result<AccountData<VerticalZonePage>> {
        validate_page_code(page_code)?;
        entry.validate()?;
        let token = self.playlist_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let sequence = self.vertical_zone.enter(&token, &api)?;
        let groups = match self.verified_pc_experiment_groups(epoch).await {
            Ok(value) => Some(value),
            Err(error)
                if error.is_service_code(301)
                    || matches!(error, Error::StaleResponse | Error::CredentialChanged) =>
            {
                return Err(error);
            }
            Err(_) => None,
        };
        let assigned = groups
            .as_ref()
            .and_then(|groups| groups.group("PH-PC-zhuanquTab"))
            == Some("t1");
        let (owner, bound) = (&token, &api);
        let mut changes = self.session.subscribe()?;
        let value = self
            .session
            .run_for(owner, |current, _| async move {
                self.session.require_api(owner, &current)?;
                self.session.require_api(owner, bound)?;
                let account = AccountApi::new(current.clone());
                let privacy = tokio::select! {
                    biased;
                    _ = changes.changed() => return Err(Error::StaleResponse),
                    response = account.settings() => response?,
                };
                self.session.require_api(owner, &current)?;
                let enabled = match privacy["personalServiceSetting"].as_i64() {
                    Some(0) => true,
                    Some(2) => false,
                    _ => {
                        return Err(Error::Response(
                            "vertical zone privacy preference unavailable",
                        ));
                    }
                };
                if let Some(page) = self
                    .vertical_zone
                    .load(sequence, page_code, &entry, enabled, assigned)?
                {
                    return Ok(page);
                }
                let page = tokio::select! {
                    biased;
                    _ = changes.changed() => return Err(Error::StaleResponse),
                    response = read(current.as_ref(), groups.as_ref(), enabled, page_code, entry) => response?,
                };
                self.session
                    .with_verified_profile(owner, &current, |_, _| {
                        self.vertical_zone
                            .finish(sequence, page_code, enabled, &page)
                    })?;
                Ok(page)
            })
            .await?;
        self.session.require_api(&token, &api)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}

#[cfg(test)]
mod tests;
