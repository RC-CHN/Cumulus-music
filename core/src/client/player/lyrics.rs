mod segment;
use super::*;
use crate::lyrics::{LyricDemand, LyricPreferences, LyricRequest, LyricSnapshot, LyricTarget};

fn auxiliary(text: Option<&str>) -> Vec<crate::lyrics::LyricLine> {
    let text = text.unwrap_or_default();
    let yrc = crate::lyrics::parse_yrc(text).lyric_lines;
    if yrc.is_empty() {
        crate::lyrics::parse_lrc(text).lyric_lines
    } else {
        yrc
    }
}

impl MusicClient {
    async fn lyric_context(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<(SessionToken, Arc<LyricsService>, LyricTarget)> {
        let token = self.player_token(epoch)?;
        let view = self.player.controller().await?.snapshot();
        if view.account != token || view.play_id.as_deref() != Some(play_id) {
            return Err(Error::StaleResponse);
        }
        let selected = view
            .selected
            .as_ref()
            .ok_or(Error::State("no selected song"))?;
        let target = if selected.resource["local_id"].is_string() {
            let resource: crate::library::LocalResource =
                serde_json::from_value(selected.resource.clone())?;
            let context = self.local.context().await?;
            let path = tokio::task::spawn_blocking(move || {
                let item = context.library.item(&resource.local_id)?;
                let current = context.library.resource(&resource.local_id)?;
                if current.local_revision != resource.local_revision
                    || item.revision != resource.local_revision
                {
                    return Err(Error::QueueChanged);
                }
                Ok(item.path)
            })
            .await
            .map_err(|_| Error::State("local lyric worker stopped"))??;
            LyricTarget::Local(path)
        } else {
            let id = crate::protocol::api::identifier(&selected.resource["id"])?;
            if selected.resource["cloud_song_id"].as_str() == Some(id.as_str()) {
                LyricTarget::Cloud(id)
            } else {
                LyricTarget::Song(id)
            }
        };
        let mut lyrics = self.player.lyrics.lock().map_err(|_| Error::LockPoisoned)?;
        if lyrics.as_ref().is_none_or(|(old, _)| old != &token) {
            *lyrics = Some((
                token.clone(),
                Arc::new(LyricsService::new(self.session.clone())?),
            ));
        }
        Ok((
            token,
            lyrics.as_ref().expect("initialized").1.clone(),
            target,
        ))
    }
    async fn lyric_result(
        &self,
        token: &SessionToken,
        play_id: &str,
        value: LyricSnapshot,
    ) -> Result<LyricsView> {
        self.session.check_generation(token)?;
        let player = self.player.controller().await?.snapshot();
        if player.account != *token || player.play_id.as_deref() != Some(play_id) {
            return Err(Error::StaleResponse);
        }
        let lyric = value.lyric;
        let mut result = LyricsView {
            word_translation: auxiliary(lyric.yrc_info.yrc_trans.as_deref()),
            word_romanization: auxiliary(lyric.yrc_info.yrc_roma.as_deref()),
            word_lines: crate::lyrics::parse_yrc_words(
                lyric.yrc_info.yrc.as_deref().unwrap_or_default(),
            ),
            epoch: token.generation.to_string(),
            play_id: play_id.into(),
            lines: lyric.lyric_lines,
            translation: lyric.tlyric_lines,
            romanization: lyric.roma_lyric_lines,
            words: lyric.yrc_info,
            offset: lyric.offset,
            scrollable: lyric.scrollable,
            display_type: lyric.display_type,
            preferences: value.preferences,
            network_failed: value.network_failed,
            from_cache: value.from_cache,
        };
        if let Some(timeline) = &player.timeline
            && timeline.mode == crate::playback::TimelineMode::Cue
        {
            segment::apply(&mut result, timeline.media_start, timeline.length);
        }
        Ok(result)
    }
    pub async fn player_lyrics(&self, epoch: &str, play_id: &str) -> Result<LyricsView> {
        let (token, service, target) = self.lyric_context(epoch, play_id).await?;
        if matches!(target, LyricTarget::Local(_)) || service.snapshot()?.network_failed {
            service.clear()?;
        }
        let value = service
            .fetch(LyricRequest {
                target,
                demand: LyricDemand {
                    detail_page: true,
                    ..Default::default()
                },
                downloaded_audio: None,
                track_mv: false,
            })
            .await?;
        self.lyric_result(&token, play_id, value).await
    }
    pub(in crate::client) async fn cached_player_lyrics(
        &self,
        epoch: &str,
        play_id: &str,
        observed: Option<&str>,
    ) -> Result<Option<(String, LyricsView, bool)>> {
        let (token, service, target) = self.lyric_context(epoch, play_id).await?;
        let Some((revision, value)) = service.snapshot_if_changed(&target, observed)? else {
            return Ok(None);
        };
        let failed = value.is_lyric_fetch_failed;
        Ok(Some((
            revision,
            self.lyric_result(&token, play_id, value).await?,
            failed,
        )))
    }
    pub async fn lyric_preferences(
        &self,
        epoch: &str,
        play_id: &str,
        preferences: LyricPreferences,
    ) -> Result<LyricsView> {
        let (token, service, target) = self.lyric_context(epoch, play_id).await?;
        if service.snapshot()?.resource_key.as_deref() != Some(&target.resource_key()?) {
            return Err(Error::StaleResponse);
        }
        let value = service.set_preferences(preferences)?;
        self.lyric_result(&token, play_id, value).await
    }
    pub async fn lyric_offset(&self, epoch: &str, play_id: &str, gain: f64) -> Result<LyricsView> {
        let (token, service, target) = self.lyric_context(epoch, play_id).await?;
        if service.snapshot()?.resource_key.as_deref() != Some(&target.resource_key()?) {
            return Err(Error::StaleResponse);
        }
        let value = service.adjust_offset(gain)?;
        self.lyric_result(&token, play_id, value).await
    }
}
