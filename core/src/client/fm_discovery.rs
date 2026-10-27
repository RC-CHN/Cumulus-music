use super::{MusicClient, dto::AccountData, mapping};
use crate::{
    Error, Result,
    fm::discovery::{FmDiscovery, FmDiscoveryQuery, SavedFmDiscovery},
};
impl MusicClient {
    pub async fn fm_discovery(
        &self,
        epoch: &str,
        query: FmDiscoveryQuery,
        refresh: bool,
    ) -> Result<AccountData<SavedFmDiscovery>> {
        let token = self.player_token(epoch)?;
        let service = FmDiscovery::new(self.session.clone())?;
        let mut saved = if refresh {
            service.page_for(&token, &query).await?
        } else {
            service
                .cached_for(&token, &query)?
                .ok_or(Error::State("FM discovery is not cached"))?
        };
        for card in &mut saved.page.cards {
            card.cover_url = card
                .cover_url
                .as_ref()
                .and_then(|s| mapping::artwork(&serde_json::json!(s)));
        }
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: saved,
            })
        })
    }
}
