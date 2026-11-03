use super::{MusicClient, dto::AccountData};
use crate::{
    Result,
    account::profile::{AccountProfile, SelfProfileDetailView},
};

impl MusicClient {
    pub async fn account_profile_detail(
        &self,
        epoch: &str,
    ) -> Result<AccountData<SelfProfileDetailView>> {
        let token = self.player_token(epoch)?;
        let value = AccountProfile::new(self.session.clone())
            .read_for(&token)
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
