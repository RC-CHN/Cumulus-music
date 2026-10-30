use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    catalog::search::{self, SearchDiscovery, SearchDiscoveryQuery},
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchHistoryCommand {
    Read,
    Remember { keyword: String },
    Remove { keyword: String },
    Clear,
}

impl MusicClient {
    pub async fn search_discovery(
        &self,
        epoch: &str,
        query: SearchDiscoveryQuery,
    ) -> Result<AccountData<SearchDiscovery>> {
        let token = self.player_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, _| search::discover(api, query))
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
        self.session.with_token(&token, |uid| {
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
        })
    }
}
