mod audio_toplist;
mod correction_channel;
mod default_context;
pub(crate) mod experiment;
mod group_download;
mod guest;
pub use group_download::{
    SearchGroupDownloadCommand, SearchGroupDownloadDecision, SearchGroupDownloadReply,
    SearchGroupDownloadRow, SearchGroupDownloadTarget,
};
mod lyrics;
pub(super) mod podcast_playback;
mod presentation;
mod privilege;
pub use presentation::{
    SearchPresentationItem, SearchPresentationPatch, SearchPresentationSection,
};
pub use privilege::SearchPrivilegePatch;
pub(super) mod queue_drop;
pub use queue_drop::{SearchDropRow, SearchNextRight};
mod reason;
mod results;
pub use reason::{ReasonTarget, SearchReasonPage};
mod track_hint;
mod voice_menu;
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    catalog::search::{self, SearchDiscovery, SearchDiscoveryQuery},
};
pub use results::*;
use serde::Deserialize;
use serde_json::json;
pub use voice_menu::{SearchVoiceMenu, SearchVoiceMenuTarget};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchHistoryCommand {
    Read,
    Remember { keyword: String },
    Remove { keyword: String },
    Clear,
}

impl MusicClient {
    pub(super) fn search_drop_rows(
        &self,
        token: &crate::account::SessionToken,
        rows: &[SearchDropRow],
    ) -> Result<Vec<super::dto::TrackView>> {
        self.session
            .with_token(token, |_| self.search.queue_drops.resolve(token, rows))
    }

    pub async fn search_discovery(
        &self,
        epoch: &str,
        query: SearchDiscoveryQuery,
    ) -> Result<AccountData<SearchDiscovery>> {
        let token = self.player_token(epoch)?;
        if token.uid.is_none() {
            return Ok(AccountData {
                epoch: epoch.into(),
                value: self.guest_search_discovery(&token, query).await?,
            });
        }
        let token = &token;
        let value = self
            .session
            .run_for(token, |api, _| async move {
                if matches!(query, SearchDiscoveryQuery::Defaults) {
                    let revision = api.search_default_context_revision().ok().flatten();
                    let (rows, refresh_ms) = search::fetch_default_words(api.as_ref()).await?;
                    let words = self.search.default_contexts.publish(
                        &self.session,
                        token,
                        &api,
                        revision.as_deref(),
                        rows,
                    )?;
                    return Ok(SearchDiscovery::Defaults { words, refresh_ms });
                }
                search::discover_for(&api, query, || {
                    self.search
                        .audio_toplist
                        .assigned(&self.session, token, &api)
                })
                .await
            })
            .await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub fn search_history(
        &self,
        epoch: &str,
        command: SearchHistoryCommand,
    ) -> Result<AccountData<Vec<String>>> {
        let token = self.player_token(epoch)?;
        let store = self.session.persistent_store()?;
        let history = |uid: &str| {
            let value = store.transaction(|tx| {
                if matches!(command, SearchHistoryCommand::Clear) {
                    tx.put(uid, "search-history-v1", "keywords", &json!([]))?;
                    return Ok(vec![]);
                }
                let raw = tx
                    .get(uid, "search-history-v1", "keywords")?
                    .unwrap_or_else(|| json!([]));
                let rows: Vec<String> = serde_json::from_value(raw)
                    .map_err(|_| Error::State("invalid search history"))?;
                let mut words = Vec::new();
                for row in rows {
                    let word = search::keyword(&row)?;
                    if !words.contains(&word) && words.len() < 30 {
                        words.push(word);
                    }
                }
                let write = !matches!(command, SearchHistoryCommand::Read);
                match command {
                    SearchHistoryCommand::Read => {}
                    SearchHistoryCommand::Remember { keyword } => {
                        let word = search::keyword(&keyword)?;
                        words.retain(|w| w != &word);
                        words.insert(0, word);
                        words.truncate(30);
                    }
                    SearchHistoryCommand::Remove { keyword } => {
                        let word = search::keyword(&keyword)?;
                        words.retain(|w| w != &word);
                    }
                    SearchHistoryCommand::Clear => words.clear(),
                }
                if write {
                    tx.put(uid, "search-history-v1", "keywords", &json!(words))?;
                }
                Ok(words)
            })?;
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        };
        if token.uid.is_none() {
            self.guest_search(&token)?.with_local_history(history)
        } else {
            self.session.with_token(&token, history)
        }
    }
}
