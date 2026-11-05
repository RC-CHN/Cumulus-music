//! ZoneFmCard.tsx: passive cover projection from the native first track.
use super::{AccountData, MusicClient, Registry, ZoneAuthority, ZoneHost, ZoneTarget};
use crate::{
    Error, Result,
    catalog::vertical_zone::ZoneBlockKind,
    fm::{FmApi, FmTrack},
    protocol::artwork,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneFmPreview {
    /// Absent keeps the card's existing server artwork. No media, song
    /// metadata, algorithm, privilege or rule receipt crosses this boundary.
    pub cover_url: Option<String>,
}

struct ZoneRule {
    id: String,
    preferred_trial: Option<i32>,
}

impl ZoneHost {
    fn fm_preview_rule(
        &self,
        authority: &ZoneAuthority,
        registry: Option<&Registry>,
        view: &str,
        card_key: &str,
    ) -> Result<ZoneRule> {
        let cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
        let authorized = cache
            .owner
            .as_ref()
            .is_some_and(|binding| authority.matches(binding));
        let (_, page) = cache
            .published
            .as_ref()
            .filter(|(page_code, page)| {
                authorized && page.view_id == view && authority.permits(registry, page_code)
            })
            .ok_or(Error::StaleResponse)?;
        page.blocks
            .iter()
            .filter(|block| {
                block.kind == ZoneBlockKind::Horizontal && !block.requires_recommendation
            })
            .flat_map(|block| &block.cards)
            .filter(|card| card.card_key == card_key && card.can_play)
            .find_map(|card| match &card.target {
                ZoneTarget::Fm { id, personalized } => Some(ZoneRule {
                    id: id.clone(),
                    preferred_trial: personalized.then_some(121),
                }),
                _ => None,
            })
            .ok_or(Error::InvalidInput("card is not an active zone FM rule"))
    }
}

fn cover(track: &FmTrack) -> Option<String> {
    let album = track.song.get("album").or_else(|| track.song.get("al"))?;
    let picture = album.get("picUrl")?;
    if picture.as_str()?.len() > 2048 {
        return None;
    }
    artwork::square(picture)
}

impl MusicClient {
    async fn verify_zone_fm_card(
        &self,
        authority: &ZoneAuthority,
        view: &str,
        card_key: &str,
    ) -> Result<ZoneRule> {
        let registry = self.vertical_zone.registry.lock().await;
        self.session
            .with_verified_profile(&authority.token, &authority.api, |_, _| {
                authority.check_revision()?;
                self.vertical_zone
                    .fm_preview_rule(authority, registry.as_ref(), view, card_key)
            })
    }

    pub async fn vertical_zone_fm_preview(
        &self,
        epoch: &str,
        view: &str,
        card_key: &str,
    ) -> Result<AccountData<ZoneFmPreview>> {
        if view.len() > 20 || card_key.len() > 20 {
            return Err(Error::InvalidInput("invalid zone FM card receipt"));
        }
        let authority = self
            .vertical_zone_authority(epoch)
            .await?
            .ok_or(Error::StaleResponse)?;
        let owner = &authority.token;
        let api = &authority.api;
        let rule = self.verify_zone_fm_card(&authority, view, card_key).await?;
        let service = self.player.fm_service().await?;
        let mut changes = self.session.subscribe()?;
        self.session.require_api(owner, api)?;
        let _rule = tokio::select! {
            biased;
            _ = changes.changed() => return Err(Error::StaleResponse),
            gate = service.zone_rule_gate(owner, &rule.id) => gate,
        };
        self.verify_zone_fm_card(&authority, view, card_key).await?;
        if let Some(first) = service.cached_zone_first(
            owner,
            api,
            &authority.revision,
            &rule.id,
            rule.preferred_trial,
        )? {
            self.verify_zone_fm_card(&authority, view, card_key).await?;
            return Ok(AccountData {
                epoch: epoch.into(),
                value: ZoneFmPreview {
                    cover_url: cover(&first),
                },
            });
        }
        let tracks = self
            .session
            .run_for(owner, |current, _| async {
                self.session.require_api(owner, &current)?;
                self.session.require_api(owner, api)?;
                authority.check_revision()?;
                let fm = FmApi::new(current);
                tokio::select! {
                    biased;
                    _ = changes.changed() => Err(Error::StaleResponse),
                    batch = fm.zone_preview_batch(&rule.id) => batch,
                }
            })
            .await?;
        let cover_url = tracks.first().and_then(cover);
        let registry = self.vertical_zone.registry.lock().await;
        self.session.with_verified_profile(owner, api, |_, _| {
            authority.check_revision()?;
            self.vertical_zone
                .fm_preview_rule(&authority, registry.as_ref(), view, card_key)?;
            service.remember_zone_preview(owner, api, &authority.revision, rule.id, tracks)
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: ZoneFmPreview { cover_url },
        })
    }
}

#[cfg(test)]
mod tests;
