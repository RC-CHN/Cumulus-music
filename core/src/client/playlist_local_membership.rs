mod addition;
pub use addition::{PlaylistMixedBatchCollection, PlaylistMixedBatchSource};

use super::{MusicClient, dto::AccountData};
use crate::{
    Result,
    playlist::{
        PlaylistLocalChange, PlaylistLocalMutation, PlaylistLocalStore, PlaylistMixedOrderRow,
        PlaylistMixedRemoveReceipt, PlaylistMixedReorderReceipt,
    },
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

    pub async fn reorder_playlist_mixed(
        &self,
        epoch: &str,
        id: &str,
        expected_revision: &str,
        rows: Vec<PlaylistMixedOrderRow>,
    ) -> Result<AccountData<PlaylistMixedReorderReceipt>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .reorder_mixed(&token, id, expected_revision, rows)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub fn playlist_mixed_reorder_receipt(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<Option<PlaylistMixedReorderReceipt>>> {
        let token = self.playlist_token(epoch)?;
        let value =
            PlaylistLocalStore::new(self.session.clone())?.mixed_reorder_receipt(&token, id)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn move_playlist_mixed(
        &self,
        epoch: &str,
        id: &str,
        expected_revision: &str,
        selected: Vec<PlaylistMixedOrderRow>,
        gap: usize,
    ) -> Result<AccountData<PlaylistMixedReorderReceipt>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .move_mixed(&token, id, expected_revision, selected, gap)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn reconcile_playlist_mixed_reorder(
        &self,
        epoch: &str,
        id: &str,
        operation_id: &str,
    ) -> Result<AccountData<PlaylistMixedReorderReceipt>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .reconcile_mixed_reorder(&token, id, operation_id)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn remove_playlist_mixed(
        &self,
        epoch: &str,
        id: &str,
        expected_revision: &str,
        selected: Vec<PlaylistMixedOrderRow>,
    ) -> Result<AccountData<PlaylistMixedRemoveReceipt>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .remove_mixed(&token, id, expected_revision, selected)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub fn playlist_mixed_remove_receipt(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<Option<PlaylistMixedRemoveReceipt>>> {
        let token = self.playlist_token(epoch)?;
        let value =
            PlaylistLocalStore::new(self.session.clone())?.mixed_remove_receipt(&token, id)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn reconcile_playlist_mixed_remove(
        &self,
        epoch: &str,
        id: &str,
        operation_id: &str,
    ) -> Result<AccountData<PlaylistMixedRemoveReceipt>> {
        let token = self.playlist_token(epoch)?;
        let value = PlaylistLocalStore::new(self.session.clone())?
            .reconcile_mixed_remove(&token, id, operation_id)
            .await?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
