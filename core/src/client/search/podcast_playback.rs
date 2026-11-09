//! Process-local authority for comprehensive AudioCards and classified Audio rows.
use super::{SearchCategory, SearchItem, SearchResultView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::MusicClient,
    playback::engine::Cancellation,
    podcast::Radio,
    protocol::api::{Api, identifier},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const PAGE_LIMIT: usize = 16;
const RADIO_LIMIT: usize = 128;
const LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum PodcastPlaybackLane {
    Card,
    Row,
}

struct Page {
    lane: PodcastPlaybackLane,
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    issued: Instant,
    cards: BTreeMap<String, Radio>,
}

impl Page {
    fn owns(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: Option<&str>) -> bool {
        self.token == *token
            && self.api.ptr_eq(&Arc::downgrade(api))
            && self.credentials.as_deref() == credentials
            && self.issued.elapsed() < LIFETIME
    }
}

#[derive(Default, Clone)]
pub(super) struct SearchPodcastPlaybacks(Arc<Mutex<VecDeque<Page>>>);

impl SearchPodcastPlaybacks {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<String>,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let lane = match view.query.category {
            SearchCategory::All => PodcastPlaybackLane::Card,
            SearchCategory::Podcasts => PodcastPlaybackLane::Row,
            _ => return Ok(()),
        };
        if api.search_privilege_credential_revision().ok() != Some(credentials.clone()) {
            return Ok(());
        }
        let mut radios = BTreeMap::<String, Option<Radio>>::new();
        let observed = |section: &super::SearchSection| match lane {
            PodcastPlaybackLane::Row => section.category == Some(SearchCategory::Podcasts),
            PodcastPlaybackLane::Card => matches!(
                section.id.rsplit_once(':').map(|(code, _)| code),
                Some(
                    "search_block_best_match"
                        | "search_block_hot_result_core_resources"
                        | "search_block_voicelist"
                )
            ),
        };
        for section in view.sections.iter().filter(|section| observed(section)) {
            for item in &section.items {
                let SearchItem::Podcast {
                    podcast,
                    playback_category,
                    ..
                } = item
                else {
                    continue;
                };
                if podcast.unavailable || podcast.id.len() > 32 {
                    continue;
                }
                if let Some(current) = radios.get_mut(&podcast.id) {
                    if current.as_ref().is_some_and(|current| {
                        current.raw["categoryId"].as_i64() != *playback_category
                    }) {
                        *current = None;
                    }
                } else if radios.len() < RADIO_LIMIT {
                    // Keep just the bounded public identity needed when a
                    // program row omits its parent. No raw result in this shelf.
                    let radio = Radio::from_official(&json!({
                        "id": podcast.id,
                        "name": podcast.name.chars().take(256).collect::<String>(),
                        "categoryId": playback_category,
                    }))?;
                    radios.insert(podcast.id.clone(), Some(radio));
                }
            }
        }
        let radios = radios
            .into_iter()
            .filter_map(|(id, radio)| radio.map(|radio| (id, radio)))
            .collect::<BTreeMap<_, _>>();
        if radios.is_empty() {
            return Ok(());
        }
        let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        pages.retain(|page| page.owns(token, api, credentials.as_deref()));
        if pages.len() >= PAGE_LIMIT {
            pages.pop_front();
        }
        let mut cards = BTreeMap::new();
        for section in view.sections.iter_mut().filter(|section| observed(section)) {
            for item in &mut section.items {
                if let SearchItem::Podcast {
                    podcast,
                    playback_receipt,
                    ..
                } = item
                    && !podcast.unavailable
                    && cards.len() < RADIO_LIMIT
                    && let Some(radio) = radios.get(&podcast.id)
                {
                    let receipt = uuid::Uuid::new_v4().simple().to_string();
                    cards.insert(receipt.clone(), radio.clone());
                    *playback_receipt = Some(receipt);
                }
            }
        }
        pages.push_back(Page {
            lane,
            token: token.clone(),
            api: Arc::downgrade(api),
            credentials,
            issued: Instant::now(),
            cards,
        });
        Ok(())
    }

    fn resolve(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        lane: PodcastPlaybackLane,
        receipt: &str,
        id: &str,
    ) -> Result<Radio> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .filter(|page| page.lane == lane && page.owns(token, api, credentials))
            .find_map(|page| {
                page.cards
                    .get(receipt)
                    .filter(|radio| radio.id == id)
                    .cloned()
            })
            .ok_or(Error::QueueChanged)
    }

    fn still_observed(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        lane: PodcastPlaybackLane,
        receipt: &str,
    ) -> bool {
        self.0.lock().is_ok_and(|pages| {
            pages.iter().any(|page| {
                page.lane == lane
                    && page.cards.contains_key(receipt)
                    && page.owns(token, api, credentials)
            })
        })
    }
}

impl MusicClient {
    pub(in crate::client) fn search_podcast_playback_source(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        lane: PodcastPlaybackLane,
        receipt: &str,
        id: &str,
    ) -> Result<(Radio, Cancellation)> {
        if receipt.len() != 32 || !receipt.bytes().all(|c| c.is_ascii_hexdigit()) || id.len() > 32 {
            return Err(Error::InvalidInput(
                "invalid search podcast playback source",
            ));
        }
        identifier(&json!(id))?;
        self.session.require_api(token, api)?;
        let credentials = api.search_privilege_credential_revision()?;
        let pages = self.search.podcast_playbacks.clone();
        let radio = pages.resolve(token, api, credentials.as_deref(), lane, receipt, id)?;
        let (session, token, api, receipt) = (
            self.session.clone(),
            token.clone(),
            api.clone(),
            receipt.to_owned(),
        );
        let guard = Cancellation::new(move || {
            session.require_api(&token, &api).is_ok()
                && api.search_privilege_credential_revision().ok().as_ref() == Some(&credentials)
                && pages.still_observed(&token, &api, credentials.as_deref(), lane, &receipt)
        });
        Ok((radio, guard))
    }
}
