//! M/3089 PlaylistCard cover and M/537 RoundPlayButton use different FM paths.
use super::{AccountData, MusicClient};
use crate::{
    Error, Result,
    client::{dto::PlayerView, focus::Owner, player::project},
    fm::{
        FmSource,
        player::{ZoneCardStart, source},
    },
    membership::Quality,
    playback::controller::PlaybackStatus,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ZoneFmCardGesture {
    Cover,
    Overlay,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneFmCardResult {
    pub playback: PlayerView,
    pub detail_play_id: Option<String>,
}

impl MusicClient {
    pub async fn vertical_zone_fm_card(
        &self,
        epoch: &str,
        view: &str,
        card_key: &str,
        gesture: ZoneFmCardGesture,
        quality: Quality,
    ) -> Result<AccountData<ZoneFmCardResult>> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let revision = api
            .verified_pc_experiment_revision()?
            .ok_or(Error::StaleResponse)?;
        self.verify_local_zone_fm_card(&token, &api, &revision, view, card_key)?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        let is_cover = gesture == ZoneFmCardGesture::Cover;
        if is_cover
            && before.state == PlaybackStatus::Playing
            && let Some(play_id) = before.play_id.clone()
            && matches!(source(&queue)?, Some(FmSource::Zone { .. }))
        {
            // The original cover opens *the already playing* Zone FM even
            // when this card represents another rule. Local receipts only.
            self.verify_local_zone_fm_card(&token, &api, &revision, view, card_key)?;
            if controller.snapshot().fence() != before.fence() {
                return Err(Error::StaleResponse);
            }
            return Ok(AccountData {
                epoch: epoch.into(),
                value: ZoneFmCardResult {
                    playback: project(&before, &token)?,
                    detail_play_id: Some(play_id),
                },
            });
        }

        let authority = self
            .vertical_zone_authority(epoch)
            .await?
            .filter(|bound| bound.token == token && bound.revision == revision)
            .ok_or(Error::StaleResponse)?;
        let rule = self.verify_zone_fm_card(&authority, view, card_key)?;
        let source = FmSource::Zone {
            rule_id: rule.id,
            trial_mode: rule.preferred_trial,
        };
        if self.has_together_cleanup()? {
            return Err(Error::State("leave together before entering FM"));
        }
        let fm = self.player.fm().await?;
        let operation = fm.operation()?;
        let expected = before.fence();
        if controller.snapshot().fence() != expected {
            return Err(Error::StaleResponse);
        }
        let intent = self.player.selection.fetch_add(1, Ordering::AcqRel) + 1;
        let lease = self.claim_media(&token, Owner::Audio).await?;
        let verify = || async {
            self.focus.check(lease, Owner::Audio)?;
            self.verify_zone_fm_card(&authority, view, card_key)?;
            Ok(())
        };
        verify().await?;
        let selected = operation
            .enter_zone_card(
                &token,
                source,
                quality,
                intent,
                ZoneCardStart {
                    expected,
                    cover: is_cover,
                },
                verify,
            )
            .await?;
        self.verify_zone_fm_card(&authority, view, card_key)?;
        if controller.snapshot().fence() != selected.fence()
            || self.player.selection.load(Ordering::Acquire) != intent
        {
            return Err(Error::StaleResponse);
        }
        let detail_play_id = if is_cover
            && matches!(
                selected.state,
                PlaybackStatus::Playing | PlaybackStatus::Buffering
            ) {
            selected.play_id.clone()
        } else {
            None
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value: ZoneFmCardResult {
                playback: project(&selected, &token)?,
                detail_play_id,
            },
        })
    }
}
