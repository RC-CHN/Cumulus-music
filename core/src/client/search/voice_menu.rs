//! Native menu preparation for ordinary search voices; best matches and
//! downloadedVoice are different surfaces.
use super::{MusicClient, SearchCategory, SearchItem, SearchResultView};
use crate::{
    Error, Result,
    account::SessionToken,
    client::dto::{AccountData, SongDownloadState},
    network_diagnostics::DefaultRoute,
    playback::engine::Cancellation,
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const PAGE_LIMIT: usize = 16;
const ROW_LIMIT: usize = 128;
const LIFETIME: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchVoiceMenuTarget {
    receipt: String,
    pub(in crate::client) voice_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchVoiceMenu {
    comment_count: Option<u64>,
    download_complete: Option<bool>,
}

struct Page {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    issued: Instant,
    rows: BTreeMap<String, Row>,
}
struct Row {
    id: String,
    playable: bool,
}
impl Page {
    fn owns(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: Option<&str>) -> bool {
        self.token == *token
            && self.api.ptr_eq(&Arc::downgrade(api))
            && self.credentials.as_deref() == credentials
            && self.issued.elapsed() < LIFETIME
    }
}

#[derive(Clone, Default)]
pub(super) struct SearchVoiceMenuPages(Arc<Mutex<VecDeque<Page>>>);
impl SearchVoiceMenuPages {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<String>,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let category = view.query.category;
        if !matches!(category, SearchCategory::All | SearchCategory::Voices)
            || api.search_privilege_credential_revision().ok().as_ref() != Some(&credentials)
        {
            return Ok(());
        }
        let mut rows = BTreeMap::new();
        for section in &mut view.sections {
            if section.category != Some(SearchCategory::Voices)
                || category == SearchCategory::All && !section.id.starts_with("search_block_voice:")
            {
                continue;
            }
            for item in &mut section.items {
                if let SearchItem::Voice {
                    voice,
                    menu_receipt,
                    ..
                } = item
                    && voice.id.len() <= 32
                    && rows.len() < ROW_LIMIT
                {
                    let receipt = uuid::Uuid::new_v4().simple().to_string();
                    rows.insert(
                        receipt.clone(),
                        Row {
                            id: voice.id.clone(),
                            playable: !voice.unavailable,
                        },
                    );
                    *menu_receipt = Some(receipt);
                }
            }
        }
        if !rows.is_empty() {
            let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
            pages.retain(|page| page.owns(token, api, credentials.as_deref()));
            if pages.len() >= PAGE_LIMIT {
                pages.pop_front();
            }
            pages.push_back(Page {
                token: token.clone(),
                api: Arc::downgrade(api),
                credentials,
                issued: Instant::now(),
                rows,
            });
        }
        Ok(())
    }

    fn require(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        target: &SearchVoiceMenuTarget,
        playable: bool,
    ) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .any(|page| {
                page.owns(token, api, credentials)
                    && page
                        .rows
                        .get(&target.receipt)
                        .is_some_and(|row| row.id == target.voice_id && (!playable || row.playable))
            })
            .then_some(())
            .ok_or(Error::QueueChanged)
    }
}

impl MusicClient {
    fn require_search_voice_menu(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        target: &SearchVoiceMenuTarget,
        playable: bool,
    ) -> Result<()> {
        if self.login_pending()?
            || api.search_privilege_credential_revision()?.as_deref() != credentials
        {
            return Err(Error::StaleResponse);
        }
        self.session.require_api(token, api)?;
        self.search
            .voice_menu_pages
            .require(token, api, credentials, target, playable)
    }

    fn search_voice_menu_source(
        &self,
        token: &SessionToken,
        target: &SearchVoiceMenuTarget,
        playable: bool,
    ) -> Result<(Arc<dyn Api>, Option<String>)> {
        if target.receipt.len() != 32
            || !target.receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
            || target.voice_id.len() > 32
        {
            return Err(Error::InvalidInput("invalid search voice menu target"));
        }
        identifier(&json!(target.voice_id))?;
        let api = self.session.native_api(token)?;
        let credentials = api.search_privilege_credential_revision()?;
        self.require_search_voice_menu(token, &api, credentials.as_deref(), target, playable)?;
        Ok((api, credentials))
    }

    pub(in crate::client) fn search_voice_menu_playback_source(
        &self,
        token: &SessionToken,
        target: &SearchVoiceMenuTarget,
    ) -> Result<(Arc<dyn Api>, Cancellation)> {
        let (api, credentials) = self.search_voice_menu_source(token, target, true)?;
        let pages = self.search.voice_menu_pages.clone();
        let (session, token, bound, target, pending) = (
            self.session.clone(),
            token.clone(),
            api.clone(),
            target.clone(),
            self.auth.pending_guard(),
        );
        let guard = Cancellation::new(move || {
            pending().is_ok()
                && session.require_api(&token, &bound).is_ok()
                && bound.search_privilege_credential_revision().ok().as_ref() == Some(&credentials)
                && pages
                    .require(&token, &bound, credentials.as_deref(), &target, true)
                    .is_ok()
        });
        Ok((api, guard))
    }

    pub async fn search_voice_menu(
        &self,
        epoch: &str,
        target: SearchVoiceMenuTarget,
        route: DefaultRoute,
    ) -> Result<AccountData<SearchVoiceMenu>> {
        let token = self.player_token(epoch)?;
        let (api, credentials) = self.search_voice_menu_source(&token, &target, false)?;
        let current =
            || self.require_search_voice_menu(&token, &api, credentials.as_deref(), &target, false);
        let comment_count = self
            .read_program_comment_count(&token, &target.voice_id, route)
            .await;
        current()?;
        let download_complete = self
            .voice_download_state(epoch, &target.voice_id)
            .await
            .ok()
            .map(|status| status == SongDownloadState::Complete);
        current()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: SearchVoiceMenu {
                comment_count,
                download_complete,
            },
        })
    }

    /// The selected path is exclusively for the native desktop shell.
    pub async fn search_voice_directory_path(
        &self,
        epoch: &str,
        target: SearchVoiceMenuTarget,
    ) -> Result<PathBuf> {
        let token = self.player_token(epoch)?;
        let (api, credentials) = self.search_voice_menu_source(&token, &target, false)?;
        let path = self
            .completed_voice_directory(epoch, &target.voice_id)
            .await?;
        self.require_search_voice_menu(&token, &api, credentials.as_deref(), &target, false)?;
        Ok(path)
    }
}
