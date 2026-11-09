use super::{MusicClient, dto::AccountData};
use crate::{
    Result,
    playlist::{PlaylistLocalChange, PlaylistLocalMutation, PlaylistLocalStore},
};

impl MusicClient {
    pub async fn mutate_playlist_local(
        &self,
        epoch: &str,
        id: &str,
        expected_revision: &str,
        change: PlaylistLocalChange,
    ) -> Result<AccountData<PlaylistLocalMutation>> {
        let token = self.playlist_token(epoch)?;
        let context = self.local.context().await?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .mutate(&token, id, expected_revision, change, &context.library)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
