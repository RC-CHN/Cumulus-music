//! Registered anonymous catalog reads. This adapter carries no authenticated
//! UID or other Api capabilities and cannot bootstrap a guest on demand.
use super::{
    MusicClient, SearchItem, SearchResultView,
    results::{empty_title, project},
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    catalog::search::{self, SearchDiscovery, SearchDiscoveryQuery, results as catalog},
    protocol::{
        api::{Api, ApiFuture},
        http::{GuestSearchAuthority, MusicHttpClient},
    },
};
use serde_json::Value;
use std::{any::Any, sync::Arc};

pub(super) struct GuestSearch {
    session: Arc<AccountSession>,
    token: SessionToken,
    api: Arc<dyn Api>,
    transport: Arc<MusicHttpClient>,
    authority: GuestSearchAuthority,
}

impl GuestSearch {
    fn check_native(&self) -> Result<()> {
        if self.transport.guest_search_authority()?.as_ref() != Some(&self.authority) {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }

    fn check(&self) -> Result<()> {
        self.check_session()?;
        self.check_native()
    }

    fn check_session(&self) -> Result<()> {
        if !self.session.guest_ready_for(&self.token, &self.api)? {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    pub(super) fn with_local_history<T>(
        &self,
        action: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        self.session
            .with_pc_presentation_identity(&self.token, &self.api, |_, _, guest_ready| {
                if !guest_ready {
                    return Err(Error::StaleResponse);
                }
                self.check_native()?;
                // Separate from every real account; never pass the private UID.
                action("local-guest")
            })
    }
}

impl Api for GuestSearch {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if cache {
                return Err(Error::InvalidInput("guest search requires a live read"));
            }
            self.transport
                .read_guest_search(path, payload, &self.authority, &|| self.check_session())
                .await
        })
    }

    fn request_search_with_default_context<'a>(
        &'a self,
        path: &'a str,
        payload: Value,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if revision != self.authority.revision
                || path != catalog::COMPREHENSIVE_PATH
                || payload["channel"] != "defaultquery"
            {
                return Err(Error::StaleResponse);
            }
            self.request(path, payload, false).await
        })
    }

    fn request_search_with_correction_channel<'a>(
        &'a self,
        path: &'a str,
        payload: Value,
        revision: &'a str,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if revision != self.authority.revision
                || path == catalog::v3::PATH
                || !catalog::correction_request(path, &payload)
            {
                return Err(Error::StaleResponse);
            }
            self.request(path, payload, false).await
        })
    }
}

impl MusicClient {
    pub(super) fn guest_search(&self, token: &SessionToken) -> Result<GuestSearch> {
        let api = self.session.native_api(token)?;
        if !self.session.guest_ready_for(token, &api)? {
            return Err(unavailable());
        }
        let transport: Arc<dyn Any + Send + Sync> = api.clone();
        let transport = transport
            .downcast::<MusicHttpClient>()
            .map_err(|_| unavailable())?;
        let authority = transport
            .guest_search_authority()?
            .ok_or_else(unavailable)?;
        Ok(GuestSearch {
            session: self.session.clone(),
            token: token.clone(),
            api,
            transport,
            authority,
        })
    }

    pub(super) async fn guest_search_discovery(
        &self,
        token: &SessionToken,
        query: SearchDiscoveryQuery,
    ) -> Result<SearchDiscovery> {
        if matches!(query, SearchDiscoveryQuery::ChartSongs { .. }) {
            return Err(Error::InvalidInput(
                "guest search does not authorize chart playback",
            ));
        }
        let guest = Arc::new(self.guest_search(token)?);
        let mut value = if matches!(query, SearchDiscoveryQuery::Defaults) {
            let (rows, refresh_ms) = search::fetch_default_words(guest.as_ref())
                .await
                .map_err(guest_error)?;
            guest.check()?;
            let words = self.search.guest_default_contexts.publish(
                &self.session,
                token,
                &guest.api,
                Some(&guest.authority.revision),
                rows,
            )?;
            SearchDiscovery::Defaults { words, refresh_ms }
        } else {
            let reader: Arc<dyn Api> = guest.clone();
            search::discover_for(&reader, query, || async { Ok(false) })
                .await
                .map_err(guest_error)?
        };
        match &mut value {
            SearchDiscovery::Charts { charts } => {
                for chart in charts {
                    chart.need_play = false;
                    chart.play_keywords = None;
                }
            }
            SearchDiscovery::Chart { play_keywords, .. } => *play_keywords = None,
            _ => (),
        }
        guest.check()?;
        Ok(value)
    }

    pub(super) async fn guest_search_results(
        &self,
        token: &SessionToken,
        query: catalog::SearchResultQuery,
        default_receipt: Option<&str>,
        correction_receipt: Option<&str>,
    ) -> Result<SearchResultView> {
        let keyword = catalog::validate_query(&query)?;
        let guest = self.guest_search(token)?;
        let context = if query.category == catalog::SearchCategory::All
            && query.channel == Some(catalog::SearchChannel::Defaultquery)
        {
            self.search
                .guest_default_contexts
                .resolve(&self.session, token, &guest.api, &keyword, default_receipt)?
                .map(catalog::ResultContext::Default)
        } else {
            self.search.correction_channels.resolve(
                &self.session,
                token,
                &guest.api,
                &query,
                correction_receipt,
            )?
        };
        let mut view = match catalog::request_with_context(&guest, &query, context).await {
            Ok(raw) => {
                guest.check()?;
                let (mut view, _, correction_channel) = project(raw, query, "", false)?;
                view.correction_receipt = self.search.correction_channels.capture(
                    &self.session,
                    token,
                    &guest.api,
                    Some(&guest.authority.revision),
                    view.query.keyword.trim(),
                    correction_channel,
                )?;
                view
            }
            Err(error) => match error.root() {
                Error::Service { endpoint, body, .. }
                    if query.category == catalog::SearchCategory::All
                        && endpoint == catalog::COMPREHENSIVE_PATH
                        && error.is_service_code(405) =>
                {
                    let mut view = SearchResultView::empty(query);
                    view.empty_title = Some(empty_title(body));
                    view
                }
                _ => return Err(guest_error(error)),
            },
        };
        // Keep the public presentation flag, never carry candidate rows into the guest reply.
        view.song_share_rows.clear();
        for section in &mut view.sections {
            let returned = section.items.len();
            section.items.retain_mut(|item| {
                match item {
                    SearchItem::Song {
                        queue_drop_receipt,
                        group_next_right,
                        group_download,
                        hint,
                        table_hint,
                        reasons,
                        ..
                    } => {
                        *queue_drop_receipt = None;
                        *group_next_right = None;
                        *group_download = None;
                        *hint = None;
                        *table_hint = None;
                        reasons.clear();
                    }
                    SearchItem::Playlist { playlist, .. } if playlist.private => return false,
                    SearchItem::Album { album } => album.subscribed = None,
                    SearchItem::Podcast { podcast, table, .. } => {
                        podcast.subscribed = None;
                        podcast.new_episode_count = None;
                        if let Some(table) = table {
                            table.subscription_eligible = false;
                        }
                    }
                    SearchItem::Voice { voice, .. } => voice.liked = false,
                    _ => (),
                }
                true
            });
            view.unavailable_count += returned - section.items.len();
        }
        if view.query.category == catalog::SearchCategory::All {
            view.sections.retain(|section| !section.items.is_empty());
        }
        guest.check()?;
        Ok(view)
    }
}

fn unavailable() -> Error {
    Error::State("registered guest search is unavailable")
}

fn guest_error(error: Error) -> Error {
    // Guest rejection cannot expire/recover a signed account or cause replay.
    if error.is_service_code(301) {
        unavailable()
    } else {
        error
    }
}
