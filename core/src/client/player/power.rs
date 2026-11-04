use super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    playback::keep_awake::{PlaybackKeepAwake, PlaybackPowerSink, PowerControl},
    protocol::api::Api,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

const GROUP: &str = "PH-PC-keepAwake";
const REFRESH: Duration = Duration::from_secs(30 * 60);
const RETRY_READ: Duration = Duration::from_secs(15);
const DEADLINE: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPowerView {
    pub available: bool,
    pub enabled: bool,
    /// M/70 gives the SMTC introduction precedence over this introduction.
    pub guide_deferred_for_system_media: bool,
}

pub(super) struct Assignment {
    owner: SessionToken,
    api: Arc<dyn Api>,
    read_at: Instant,
    exposed: bool,
    guide_deferred_for_system_media: bool,
    settled: bool,
}
impl Assignment {
    fn matches(&self, owner: &SessionToken, api: &Arc<dyn Api>) -> bool {
        &self.owner == owner
            && Arc::ptr_eq(&self.api, api)
            && self.read_at.elapsed() < if self.settled { REFRESH } else { RETRY_READ }
    }
}

impl MusicClient {
    /// Native desktop only. This does not initialize an audio engine.
    pub fn with_playback_power_sink(mut self, sink: Arc<dyn PlaybackPowerSink>) -> Self {
        self.player.power = Some(Arc::new(PowerControl::new(&self.player.store, sink)));
        self
    }

    pub async fn playback_power(&self, epoch: &str) -> Result<PlaybackPowerView> {
        let owner = self.player_token(epoch)?;
        let Some(control) = self.player.power.as_ref().filter(|p| p.sink.available()) else {
            return Ok(PlaybackPowerView::default());
        };
        let mut changes = self.session.subscribe()?;
        let mut cached = self.player.power_assignment.lock().await;
        let api = self.session.native_api(&owner)?;
        let (available, guide_deferred_for_system_media) = if let Some(entry) =
            cached.as_ref().filter(|entry| entry.matches(&owner, &api))
        {
            (entry.exposed, entry.guide_deferred_for_system_media)
        } else {
            let result = tokio::select! {
                biased;
                _ = changes.changed() => return Err(Error::StaleResponse),
                result = tokio::time::timeout(DEADLINE, api.playback_power_experiment()) => result,
            };
            let settled = matches!(&result, Ok(Ok(_)));
            let groups = result.ok().and_then(Result::ok).flatten();
            let exposed = groups
                .as_ref()
                .and_then(|groups| groups.group(GROUP))
                .is_some_and(PlaybackKeepAwake::windows_setting_visible);
            let guide_deferred_for_system_media = groups.is_some_and(|groups| {
                groups.group("PH-PC-newSMTCoff") == Some("t1")
                    && groups.group("PH-PC-SMTC") != Some("t1")
            });
            if !Arc::ptr_eq(&api, &self.session.native_api(&owner)?) {
                return Err(Error::StaleResponse);
            }
            *cached = Some(Assignment {
                owner: owner.clone(),
                api,
                read_at: Instant::now(),
                exposed,
                guide_deferred_for_system_media,
                settled,
            });
            (exposed, guide_deferred_for_system_media)
        };
        self.session.with_local_owner(&owner, |_| {
            Ok(PlaybackPowerView {
                available: available && control.sink.available(),
                enabled: control.enabled(),
                guide_deferred_for_system_media,
            })
        })
    }

    pub async fn set_playback_power(
        &self,
        epoch: &str,
        enabled: bool,
    ) -> Result<PlaybackPowerView> {
        let owner = self.player_token(epoch)?;
        let Some(control) = self.player.power.as_ref().filter(|p| p.sink.available()) else {
            return Ok(PlaybackPowerView::default());
        };
        let cached = self.player.power_assignment.lock().await;
        let api = self.session.native_api(&owner)?;
        self.session.with_local_owner(&owner, |_| {
            let Some(entry) = cached.as_ref().filter(|entry| {
                entry.exposed && entry.matches(&owner, &api) && control.sink.available()
            }) else {
                return Ok(PlaybackPowerView {
                    enabled: control.enabled(),
                    ..PlaybackPowerView::default()
                });
            };
            control.set(&self.player.store, enabled)?;
            if let Some(controller) = self.player.controller.get() {
                controller.power_preference_changed();
            }
            Ok(PlaybackPowerView {
                available: true,
                enabled,
                guide_deferred_for_system_media: entry.guide_deferred_for_system_media,
            })
        })
    }
}
