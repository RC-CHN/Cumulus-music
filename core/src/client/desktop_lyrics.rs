//! Native-only documents share the player lyric service; clock projection is
//! lazy and local. Desktop windows cannot manufacture a document over IPC.
use super::{
    MusicClient,
    dto::LyricsView,
    focus::{FocusSnapshot, Owner},
    native_media::NativeMediaKind,
};
use crate::{
    Error, Result,
    account::SessionToken,
    lyrics::desktop::{Frame, Prepared, Source},
    preferences::desktop_lyrics::Appearance,
};
use serde::Serialize;
pub struct DesktopLyricDocument {
    token: SessionToken,
    focus: FocusSnapshot,
    play_id: String,
    revision: String,
    prepared: Prepared,
    info: DesktopLyricInfo,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLyricInfo {
    pub scrollable: bool,
    pub network_failed: bool,
    pub from_cache: bool,
    pub fetch_failed: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLyricView {
    pub epoch: String,
    pub play_id: String,
    pub revision: String,
    pub playing: bool,
    pub speed: f64,
    pub seek_revision: u64,
    pub duration: Option<f64>,
    pub info: DesktopLyricInfo,
    pub frame: Frame,
}
impl DesktopLyricDocument {
    fn check(&self, client: &MusicClient) -> Result<()> {
        client.session.check_generation(&self.token)?;
        client.focus.check_snapshot(self.focus)?;
        if client.native_shortcut_selection()?.audio_play_id.as_deref() != Some(&self.play_id) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    fn replace(&mut self, revision: String, value: LyricsView, failed: bool) {
        self.prepared = Source {
            lines: &value.lines,
            translation: &value.translation,
            romanization: &value.romanization,
            word_lines: &value.word_lines,
            word_translation: &value.word_translation,
            word_romanization: &value.word_romanization,
            offset: value.offset,
            display_type: value.display_type,
        }
        .prepare();
        self.revision = revision;
        self.info = DesktopLyricInfo {
            scrollable: value.scrollable,
            network_failed: value.network_failed,
            from_cache: value.from_cache,
            fetch_failed: failed,
        };
    }
}
impl MusicClient {
    /// Explicit resource load/retry; the same HTTP/cache/sidecar path as details.
    pub async fn desktop_lyric_document(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<DesktopLyricDocument> {
        let token = self.player_token(epoch)?;
        let focus = self.focus.snapshot()?;
        if focus.owner != Some(Owner::Audio)
            || self.native_shortcut_selection()?.audio_play_id.as_deref() != Some(play_id)
        {
            return Err(Error::StaleResponse);
        }
        self.player_lyrics(epoch, play_id).await?;
        let (revision, value, failed) = self
            .cached_player_lyrics(epoch, play_id, None)
            .await?
            .ok_or(Error::StaleResponse)?;
        let prepared = Source {
            lines: &value.lines,
            translation: &value.translation,
            romanization: &value.romanization,
            word_lines: &value.word_lines,
            word_translation: &value.word_translation,
            word_romanization: &value.word_romanization,
            offset: value.offset,
            display_type: value.display_type,
        }
        .prepare();
        let document = DesktopLyricDocument {
            token,
            focus,
            play_id: play_id.into(),
            revision,
            prepared,
            info: DesktopLyricInfo {
                scrollable: value.scrollable,
                network_failed: value.network_failed,
                from_cache: value.from_cache,
                fetch_failed: failed,
            },
        };
        document.check(self)?;
        Ok(document)
    }
    /// Observe changes made by the detail page without fetching/retrying. The
    /// unchanged path does not clone/reparse the lyric document or build indices.
    pub async fn refresh_desktop_lyric_document(
        &self,
        document: &mut DesktopLyricDocument,
    ) -> Result<bool> {
        document.check(self)?;
        let value = self
            .cached_player_lyrics(
                &document.token.generation.to_string(),
                &document.play_id,
                Some(&document.revision),
            )
            .await?;
        document.check(self)?;
        if let Some((revision, value, failed)) = value {
            document.replace(revision, value, failed);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn desktop_lyric_frame(
        &self,
        document: &DesktopLyricDocument,
        appearance: &Appearance,
    ) -> Result<DesktopLyricView> {
        document.check(self)?;
        let view = self.native_media_details()?;
        if view
            .item
            .as_ref()
            .is_none_or(|item| item.kind != NativeMediaKind::Music)
        {
            return Err(Error::StaleResponse);
        }
        let frame = document
            .prepared
            .frame(view.position.unwrap_or(0.0), appearance)?;
        view.transport
            .target
            .as_ref()
            .ok_or(Error::StaleResponse)?
            .check(self)?;
        document.check(self)?;
        Ok(DesktopLyricView {
            epoch: document.token.generation.to_string(),
            play_id: document.play_id.clone(),
            revision: document.revision.clone(),
            playing: view.transport.playing,
            speed: view.speed,
            seek_revision: view.seek_revision,
            duration: view.duration,
            info: document.info.clone(),
            frame,
        })
    }
    pub async fn desktop_lyric_offset(
        &self,
        document: &mut DesktopLyricDocument,
        gain: f64,
    ) -> Result<()> {
        document.check(self)?;
        self.lyric_offset(
            &document.token.generation.to_string(),
            &document.play_id,
            gain,
        )
        .await?;
        self.refresh_desktop_lyric_document(document).await?;
        Ok(())
    }
}
