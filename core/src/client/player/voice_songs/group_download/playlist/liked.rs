//! The system star is never accepted as an ordinary owned playlist ID.
use super::{
    AccountData, Attempt, Authority, MusicClient, PlaylistService, Target,
    VoiceSongGroupPlaylistResult, VoiceSongLikedDestination, fresh_id,
};
use crate::{Error, Result};

impl MusicClient {
    /// Observe a current same-account system row with its own opaque grant.
    pub async fn player_voice_songs_download_liked_destination(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
    ) -> Result<AccountData<Option<VoiceSongLikedDestination>>> {
        let (page, lease, _, check) = self.group_destination_scope(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Authority::SystemLiked,
        )?;
        {
            let mut state = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.attempt.is_some() {
                return Err(Error::StaleResponse);
            }
            state.liked = None;
        }
        let liked = PlaylistService::new(self.session.clone())?
            .system_liked_destination(&page.token, &check)
            .await
            .map_err(super::super::super::stale_credentials)?
            .map(|playlist_id| VoiceSongLikedDestination {
                playlist_id,
                destination_receipt: fresh_id(),
            });
        check()?;
        lease
            .0
            .destination
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .liked = liked.clone();
        Ok(AccountData {
            epoch: epoch.into(),
            value: liked,
        })
    }

    /// Revalidate the exact signed star and both sources of liked membership
    /// before one ordered playlist add. Repetition only returns its journal.
    pub async fn player_voice_songs_download_choose_liked(
        &self,
        epoch: &str,
        play_id: &str,
        card_receipt: &str,
        selection_receipt: &str,
        destination_receipt: &str,
    ) -> Result<VoiceSongGroupPlaylistResult> {
        let (page, lease, _, check) = self.group_destination_scope(
            epoch,
            play_id,
            card_receipt,
            selection_receipt,
            Authority::SystemLiked,
        )?;
        let (attempt, first) = {
            let mut state = lease
                .0
                .destination
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if let Some(attempt) = &state.attempt {
                if !matches!(&attempt.target, Target::SystemLiked { receipt, .. } if receipt == destination_receipt)
                {
                    return Err(Error::StaleResponse);
                }
                (attempt.clone(), false)
            } else {
                let observed = state
                    .liked
                    .as_ref()
                    .filter(|row| row.destination_receipt == destination_receipt)
                    .ok_or(Error::StaleResponse)?;
                let attempt = Attempt {
                    target: Target::SystemLiked {
                        id: observed.playlist_id.clone(),
                        receipt: observed.destination_receipt.clone(),
                    },
                    operation: fresh_id(),
                    continuation: fresh_id(),
                };
                state.attempt = Some(attempt.clone());
                (attempt, true)
            }
        };
        self.apply_group_playlist_choice(&page, &lease, &attempt, first, &check)
            .await
    }
}
