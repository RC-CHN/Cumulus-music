//! Browser-only contribution options recovered from PC modules 159 and 182.
use super::contributors::ordinary_song;
use crate::{
    Error, Result,
    client::MusicClient,
    lyrics::{DisplayType, LyricSnapshot, LyricTarget},
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde::Serialize;
use serde_json::Value;

const CONTRIBUTION_PAGE: &str = "https://music.163.com/#/wiki/song?type=2&songId=";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricContribution {
    Upload,
    Scrolling,
    Translation,
    ScrollingAndTranslation,
}

pub(super) fn option(
    snapshot: &LyricSnapshot,
    resource: Option<&Value>,
    target: &LyricTarget,
) -> Option<LyricContribution> {
    let LyricTarget::Song(song_id) = target else {
        return None;
    };
    let lyric = &snapshot.lyric;
    if snapshot.is_loading
        || snapshot.is_lyric_fetch_failed
        || (snapshot.network_failed && !snapshot.from_cache)
        || snapshot.from_sidecar
        || snapshot.from_embedded
        || lyric.metadata.get("isCloudLyric").is_some_and(js_truthy)
        || snapshot
            .resource_key
            .as_ref()
            .is_some_and(|key| key.strip_prefix("song:") != Some(song_id.as_str()))
        || !resource.is_some_and(|resource| ordinary_song(resource, song_id))
    {
        return None;
    }
    match lyric.display_type {
        DisplayType::Pure => None,
        DisplayType::Ask if lyric.lyric_lines.is_empty() => Some(LyricContribution::Upload),
        _ if !lyric.lyric_lines.is_empty() => {
            let ask_for_translation = lyric
                .metadata
                .get("askForTranslation")
                .is_some_and(js_truthy);
            match (lyric.scrollable, ask_for_translation) {
                (false, true) => Some(LyricContribution::ScrollingAndTranslation),
                (false, false) => Some(LyricContribution::Scrolling),
                (true, true) => Some(LyricContribution::Translation),
                (true, false) => None,
            }
        }
        _ => None,
    }
}

impl MusicClient {
    /// The host opens only the fixed public URL of the current verified song.
    /// This read of native state never fetches lyrics or submits a contribution.
    pub async fn lyric_contribution_url(&self, epoch: &str, play_id: &str) -> Result<String> {
        let context = self.lyric_context(epoch, play_id).await?;
        let Some((_, document)) = context.service.snapshot_if_changed(&context.target, None)?
        else {
            return Err(Error::StaleResponse);
        };
        let player = self.player.controller().await?.snapshot();
        if player.account != context.token || player.play_id.as_deref() != Some(play_id) {
            return Err(Error::StaleResponse);
        }
        let resource = player.selected.as_ref().map(|entry| &entry.resource);
        let song_id = match &context.target {
            LyricTarget::Song(id) if option(&document, resource, &context.target).is_some() => id,
            _ => return Err(Error::InvalidInput("lyric contribution is unavailable")),
        };
        self.session.check_generation(&context.token)?;
        public_page(song_id)
    }
}

fn public_page(song_id: &str) -> Result<String> {
    let song_id = identifier(&Value::String(song_id.into()))?;
    Ok(format!("{CONTRIBUTION_PAGE}{song_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_public_lyrics_survive_refresh_failure_but_do_not_alias_cloud_or_sidecars() {
        use serde_json::json;
        let target = LyricTarget::Song("7".into());
        let resource = json!({"id":7});
        let mut view = LyricSnapshot {
            resource_key: Some("song:7".into()),
            network_failed: true,
            from_cache: true,
            lyric: crate::lyrics::parse_lyrics(&json!({
                "askForTranslation":true,
                "lrc":{"lyric":"[00:01]合成歌词"}
            }))
            .unwrap(),
            ..Default::default()
        };
        assert_eq!(
            option(&view, Some(&resource), &target),
            Some(LyricContribution::Translation)
        );
        assert_eq!(
            option(&view, Some(&json!({"id":7,"cloud_song_id":"7"})), &target),
            None
        );
        assert_eq!(
            option(&view, Some(&resource), &LyricTarget::Song("8".into())),
            None
        );
        view.from_sidecar = true;
        assert_eq!(option(&view, Some(&resource), &target), None);
        view.from_sidecar = false;
        view.is_lyric_fetch_failed = true;
        assert_eq!(option(&view, Some(&resource), &target), None);
    }

    #[test]
    fn public_handoff_is_bound_to_an_exact_numeric_song() {
        assert_eq!(
            public_page("000184467440737095516160").unwrap(),
            "https://music.163.com/#/wiki/song?type=2&songId=000184467440737095516160"
        );
        for id in ["", "000", "4&x=5", "１", "1\n2"] {
            assert!(public_page(id).is_err(), "{id}");
        }
    }
}
