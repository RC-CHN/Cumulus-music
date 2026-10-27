use super::*;
use crate::{
    client::{MusicClient, dto::AccountData},
    playback::controller::PlaybackStatus,
    podcast::scene::SceneFmService,
};
impl MusicClient {
    pub async fn scene_fm_view(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<SceneFmView>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| Ok(()))?;
        let service = SceneFmService::new(self.session.clone())?;
        let mut state = service.cached_for(&token)?;
        let actual = self
            .player
            .controller
            .get()
            .map(|c| c.snapshot())
            .filter(|s| s.account == token);
        if let Some(s) = actual.as_ref().filter(|s| {
            matches!(
                s.state,
                PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
            )
        }) && let Some(e) = s
            .selected
            .as_ref()
            .filter(|e| e.source["kind"] == "scene_fm")
            && let (Some(scene), Some(voice)) = (
                e.source["scene_id"].as_str(),
                e.resource["voice_id"].as_str(),
            )
        {
            // Observation may overlap a selection or another observer. Return
            // the committed snapshot while its owner finishes; reads must not
            // turn a successful play into a spurious mutation-busy failure.
            let needs_sync = state.as_ref().is_some_and(|v| {
                v.pending
                    || v.active_id.as_deref() != Some(scene)
                    || v.selected().is_none_or(|v| v.id != voice)
            });
            if needs_sync {
                match service.sync_for(&token, scene, voice, e.source["scene_revision"].as_u64()) {
                    Ok(value) => state = value,
                    Err(Error::MutationBusy) => (),
                    Err(error) => return Err(error),
                }
            }
        }
        if refresh {
            state = Some(service.refresh_for(&token).await?);
        }
        let active = state.as_ref().is_some_and(|v| {
            actual
                .as_ref()
                .and_then(|a| a.selected.as_ref())
                .is_some_and(|e| {
                    e.source["kind"] == "scene_fm"
                        && e.source["scene_id"].as_str() == v.active_id.as_deref()
                        && v.selected().is_some_and(|v| e.resource["voice_id"] == v.id)
                })
        });
        let value = SceneFmView {
            revision: state.as_ref().map(|s| s.revision.to_string()),
            active_id: state.as_ref().and_then(|s| s.active_id.clone()),
            episode: state
                .as_ref()
                .and_then(|s| s.selected())
                .map(|v| super::super::podcast::view::episode(v, &[])),
            scenes: state
                .as_ref()
                .map(|s| {
                    s.scenes
                        .iter()
                        .map(|c| SceneFmCategoryView {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            label: c.label.clone(),
                            count: c.voices.len(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            active,
            playing: active
                && actual.is_some_and(|s| {
                    matches!(s.state, PlaybackStatus::Playing | PlaybackStatus::Buffering)
                }),
            pending: state.is_some_and(|s| s.pending),
            autoplay: service.autoplay_for(&token)?,
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
