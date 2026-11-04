//! Authenticated vertical-zone menu, catalogue and bounded tab cache.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::{AccountApi, SessionStatus, SessionToken},
    catalog::vertical_zone::{
        VerticalZoneMenu, VerticalZonePage, ZoneEntry, ZoneLayout, ZoneTarget, read, read_menu,
        validate_page_code,
    },
    protocol::{api::Api, pc_experiments::PcExperimentGroups},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Default)]
pub(super) struct ZoneHost {
    cache: Mutex<ZoneCache>,
    registry: AsyncMutex<Option<Registry>>,
}

#[derive(Default)]
struct ZoneCache {
    owner: Option<Binding>,
    sequence: u64,
    tabs: VecDeque<CacheItem>,
    published: Option<(String, VerticalZonePage)>,
}

struct CacheItem {
    page_code: String,
    recommendation_enabled: bool,
    page: VerticalZonePage,
}

struct Binding {
    token: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
}
impl Binding {
    fn new(token: &SessionToken, api: &Arc<dyn Api>, revision: &str) -> Self {
        Self {
            token: token.clone(),
            api: Arc::downgrade(api),
            revision: revision.into(),
        }
    }

    fn matches(&self, token: &SessionToken, api: &Arc<dyn Api>, revision: &str) -> bool {
        self.token == *token
            && self.revision == revision
            && self
                .api
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, api))
    }
}

struct Registry {
    owner: Binding,
    menu: VerticalZoneMenu,
}

struct ZoneAuthority {
    token: SessionToken,
    api: Arc<dyn Api>,
    revision: String,
    groups: PcExperimentGroups,
}
impl ZoneAuthority {
    fn matches(&self, binding: &Binding) -> bool {
        binding.matches(&self.token, &self.api, &self.revision)
    }

    fn check_revision(&self) -> Result<()> {
        if self.api.verified_pc_experiment_revision()?.as_deref() != Some(self.revision.as_str()) {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }

    fn permits(&self, registry: Option<&Registry>, page_code: &str) -> bool {
        registry.is_some_and(|value| self.matches(&value.owner) && value.menu.contains(page_code))
    }
}

fn cannot_fallback(error: &Error) -> bool {
    error.is_service_code(301) || matches!(error, Error::StaleResponse | Error::CredentialChanged)
}

impl ZoneHost {
    fn enter(&self, token: &SessionToken, api: &Arc<dyn Api>, revision: &str) -> Result<u64> {
        let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
        if !cache
            .owner
            .as_ref()
            .is_some_and(|owner| owner.matches(token, api, revision))
        {
            cache.owner = Some(Binding::new(token, api, revision));
            cache.tabs.clear();
        }
        cache.published = None;
        cache.sequence = cache
            .sequence
            .checked_add(1)
            .ok_or(Error::State("vertical zone sequence exhausted"))?;
        Ok(cache.sequence)
    }

    fn publish(&self, sequence: u64, page_code: &str, page: &mut VerticalZonePage) -> Result<()> {
        let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
        if cache.sequence != sequence {
            return Err(Error::StaleResponse);
        }
        page.view_id = sequence.to_string();
        for (block, section) in page.blocks.iter_mut().enumerate() {
            for (index, card) in section.cards.iter_mut().enumerate() {
                card.card_key = format!("{block}:{index}");
            }
        }
        cache.published = Some((page_code.into(), page.clone()));
        Ok(())
    }

    fn playlist_trial(
        &self,
        binding: &Binding,
        view: &str,
        card_key: &str,
        id: &str,
        permits: impl FnOnce(&str) -> bool,
    ) -> Result<bool> {
        let cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
        let authorized = binding.api.upgrade().is_some_and(|api| {
            cache
                .owner
                .as_ref()
                .is_some_and(|owner| owner.matches(&binding.token, &api, &binding.revision))
        });
        let (_, page) = cache
            .published
            .as_ref()
            .filter(|(page_code, page)| authorized && page.view_id == view && permits(page_code))
            .ok_or(Error::StaleResponse)?;
        page.blocks
            .iter()
            .flat_map(|block| &block.cards)
            .filter(|card| card.can_play && card.card_key == card_key)
            .find_map(|card| match &card.target {
                ZoneTarget::Playlist {
                    id: candidate,
                    personalized,
                } if candidate == id => Some(*personalized),
                _ => None,
            })
            .ok_or(Error::InvalidInput("playlist is not an active zone card"))
    }

    fn load(
        &self,
        sequence: u64,
        page_code: &str,
        entry: &ZoneEntry<'_>,
        enabled: bool,
        assigned: bool,
    ) -> Result<Option<VerticalZonePage>> {
        let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
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
        let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
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
    pub(super) async fn vertical_zone_playlist_trial(
        &self,
        token: &SessionToken,
        view: &str,
        card_key: &str,
        id: &str,
    ) -> Result<bool> {
        let api = self.session.native_api(token)?;
        let revision = api
            .verified_pc_experiment_revision()?
            .ok_or(Error::StaleResponse)?;
        let registry = self.vertical_zone.registry.lock().await;
        self.session.with_verified_profile(token, &api, |_, _| {
            if api.verified_pc_experiment_revision()?.as_deref() != Some(revision.as_str()) {
                return Err(Error::CredentialChanged);
            }
            let current = registry
                .as_ref()
                .filter(|entry| entry.owner.matches(token, &api, &revision))
                .ok_or(Error::StaleResponse)?;
            self.vertical_zone
                .playlist_trial(&current.owner, view, card_key, id, |page| {
                    current.menu.contains(page)
                })
        })
    }

    async fn vertical_zone_authority(&self, epoch: &str) -> Result<Option<ZoneAuthority>> {
        let snapshot = self.session.snapshot()?;
        let token = snapshot.token;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        if snapshot.status != SessionStatus::Authenticated {
            return Ok(None);
        }
        let api = self.session.native_api(&token)?;
        let (revision, groups) = match self.verified_pc_experiment_groups_for(&token, &api).await {
            Ok(assignment) => assignment,
            Err(error) if cannot_fallback(&error) => return Err(error),
            Err(_) => return Ok(None),
        };
        if groups.group("PH-PC-zhuanqu") != Some("t1") {
            let mut registry = self.vertical_zone.registry.lock().await;
            self.session.with_verified_profile(&token, &api, |_, _| {
                if registry
                    .as_ref()
                    .is_some_and(|entry| entry.owner.matches(&token, &api, &revision))
                {
                    *registry = None;
                }
                let mut cache = self
                    .vertical_zone
                    .cache
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?;
                if cache
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.matches(&token, &api, &revision))
                {
                    cache.tabs.clear();
                    cache.published = None;
                }
                Ok(())
            })?;
            return Ok(None);
        }
        Ok(Some(ZoneAuthority {
            token,
            api,
            revision,
            groups,
        }))
    }

    pub(super) async fn vertical_zone_search_registry(
        &self,
        epoch: &str,
    ) -> Result<Option<(PcExperimentGroups, VerticalZoneMenu)>> {
        let Some(authority) = self.vertical_zone_authority(epoch).await? else {
            return Ok(None);
        };
        let menu = self.load_zone_menu(&authority, false).await?;
        Ok(Some((authority.groups, menu)))
    }

    /// Choice reads only after the exact PC assignment. Empty/failed refreshes
    /// keep a previously verified same-authority list, as in M/2986.getTags.
    pub async fn vertical_zone_menu(&self, epoch: &str) -> Result<AccountData<VerticalZoneMenu>> {
        let value = match self.vertical_zone_authority(epoch).await? {
            Some(authority) => self.load_zone_menu(&authority, true).await?,
            None => VerticalZoneMenu::default(),
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    async fn load_zone_menu(
        &self,
        authority: &ZoneAuthority,
        refresh: bool,
    ) -> Result<VerticalZoneMenu> {
        let mut registry = self.vertical_zone.registry.lock().await;
        let (token, api) = (&authority.token, &authority.api);
        let mut changes = self.session.subscribe()?;
        self.session.require_api(token, api)?;
        authority.check_revision()?;
        let previous = registry
            .as_ref()
            .filter(|entry| authority.matches(&entry.owner))
            .map(|entry| entry.menu.clone());
        if !refresh && let Some(previous) = previous {
            return Ok(previous);
        }
        let result = self
            .session
            .run_for(token, |current, _| async move {
                self.session.require_api(token, &current)?;
                self.session.require_api(token, api)?;
                tokio::select! {
                    biased;
                    _ = changes.changed() => Err(Error::StaleResponse),
                    response = read_menu(current.as_ref()) => response,
                }
            })
            .await;
        if let Err(error) = &result
            && cannot_fallback(error)
        {
            return result;
        }
        authority.check_revision()?;
        self.session.with_verified_profile(token, api, |_, _| {
            let value = match result {
                Ok(menu) if !menu.is_empty() => menu,
                Ok(menu) => previous.unwrap_or(menu),
                Err(error) => match previous {
                    Some(entry) => entry,
                    None => return Err(error),
                },
            };
            *registry = Some(Registry {
                owner: Binding::new(token, api, &authority.revision),
                menu: value.clone(),
            });
            Ok(value)
        })
    }

    /// The renderer cannot supply an assignment, privacy bit, or upstream
    /// route. Deep links first establish the same native Choice registry.
    pub async fn vertical_zone_page(
        &self,
        epoch: &str,
        page_code: &str,
        entry: ZoneEntry<'_>,
    ) -> Result<AccountData<VerticalZonePage>> {
        validate_page_code(page_code)?;
        entry.validate()?;
        let authority = self
            .vertical_zone_authority(epoch)
            .await?
            .ok_or(Error::State(
                "vertical zones are unavailable for this account",
            ))?;
        if !self
            .load_zone_menu(&authority, false)
            .await?
            .contains(page_code)
        {
            return Err(Error::InvalidInput("unregistered vertical zone page"));
        }
        let authority = &authority;
        let (owner, bound) = (&authority.token, &authority.api);
        let sequence = self
            .vertical_zone
            .enter(owner, bound, &authority.revision)?;
        let groups = &authority.groups;
        let assigned = groups.group("PH-PC-zhuanquTab") == Some("t1");
        let mut changes = self.session.subscribe()?;
        let mut value = self
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
                authority.check_revision()?;
                {
                    let registry = self.vertical_zone.registry.lock().await;
                    if !authority.permits(registry.as_ref(), page_code) {
                        return Err(Error::StaleResponse);
                    }
                }
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
                    return Ok((page, enabled));
                }
                let page = tokio::select! {
                    biased;
                    _ = changes.changed() => return Err(Error::StaleResponse),
                    response = read(current.as_ref(), Some(groups), enabled, page_code, entry) => response?,
                };
                Ok((page, enabled))
            })
            .await?;
        let (mut value, enabled) = value;
        let registry = self.vertical_zone.registry.lock().await;
        authority.check_revision()?;
        self.session.with_verified_profile(owner, bound, |_, _| {
            if !authority.permits(registry.as_ref(), page_code) {
                return Err(Error::StaleResponse);
            }
            self.vertical_zone
                .finish(sequence, page_code, enabled, &value)?;
            self.vertical_zone.publish(sequence, page_code, &mut value)
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}

#[cfg(test)]
mod tests;
