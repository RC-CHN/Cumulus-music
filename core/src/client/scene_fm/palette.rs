//! M54/M2060 artwork color acquisition, with no renderer-selected URL.
mod artwork;
#[cfg(test)]
mod tests;
use super::revision;
use crate::{
    Error, Result,
    account::SessionToken,
    client::{MusicClient, dto::AccountData},
    download::transfer::{MediaBackend, ReqwestMediaBackend},
    podcast::scene::{
        SceneFmService,
        palette::{self, HeaderDecision, SceneFmBackground},
    },
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, SeqAccess, Visitor},
};
use std::fmt;

/// Enforce the canvas bound during deserialization, before a renderer array
/// can grow an unbounded native allocation.
pub struct SceneFmPalettePixels(Vec<u8>);
impl SceneFmPalettePixels {
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}
impl<'de> Deserialize<'de> for SceneFmPalettePixels {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Pixels;
        impl<'de> Visitor<'de> for Pixels {
            type Value = SceneFmPalettePixels;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("at most 65536 RGBA bytes")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut values: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                if values.size_hint().is_some_and(|n| n > 65_536) {
                    return Err(A::Error::custom("artwork sample exceeds limit"));
                }
                let mut pixels = Vec::with_capacity(values.size_hint().unwrap_or(0).min(65_536));
                while let Some(value) = values.next_element::<u8>()? {
                    if pixels.len() == 65_536 {
                        return Err(A::Error::custom("artwork sample exceeds limit"));
                    }
                    pixels.push(value);
                }
                Ok(SceneFmPalettePixels(pixels))
            }
        }
        deserializer.deserialize_seq(Pixels)
    }
}
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

const LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneFmPaletteRead {
    Ready {
        background: SceneFmBackground,
    },
    #[serde(rename_all = "camelCase")]
    Sample {
        receipt: String,
        mime_type: String,
        bytes: Vec<u8>,
        width: u32,
        height: u32,
    },
}
#[derive(Serialize)]
pub struct SceneFmPaletteResult {
    pub background: SceneFmBackground,
}
#[derive(Clone, PartialEq, Eq)]
struct Source {
    token: SessionToken,
    voice: String,
    cover: String,
}
/// Once a current voice has admitted the read, only its public artwork is
/// processed. Another current voice can own the identical canonical cover;
/// this does not transfer any playback, episode or account authorization.
#[derive(Clone, PartialEq, Eq)]
struct ArtworkSource {
    token: SessionToken,
    cover: String,
}
struct Pending {
    source: ArtworkSource,
    sequence: u64,
    receipt: String,
    width: u32,
    height: u32,
}
struct Cache {
    source: ArtworkSource,
    background: SceneFmBackground,
}
#[derive(Default)]
struct State {
    pending: Option<Pending>,
    cache: Option<Cache>,
}
pub(super) struct PaletteHost {
    state: Mutex<State>,
    backend: Mutex<Option<Arc<dyn MediaBackend>>>,
    sequence: watch::Sender<u64>,
}
impl Default for PaletteHost {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
            backend: Mutex::new(None),
            sequence: watch::channel(0).0,
        }
    }
}
impl PaletteHost {
    fn backend(&self) -> Result<Arc<dyn MediaBackend>> {
        let mut slot = self.backend.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(backend) = &*slot {
            return Ok(backend.clone());
        }
        let backend = Arc::new(ReqwestMediaBackend::with_artwork_metadata(
            Duration::from_secs(4),
        )?);
        *slot = Some(backend.clone());
        Ok(backend)
    }
    fn begin(&self) -> Result<watch::Receiver<u64>> {
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        let next = self
            .sequence
            .borrow()
            .checked_add(1)
            .ok_or(Error::State("artwork color generation exhausted"))?;
        state.pending = None;
        self.sequence.send_replace(next);
        Ok(self.sequence.subscribe())
    }
    fn current(&self, sequence: u64) -> Result<()> {
        if *self.sequence.borrow() == sequence {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        }
    }
}
impl MusicClient {
    /// Setup injection for closed synthetic media. It cannot alter API/session
    /// credentials or admit a renderer-supplied media address.
    pub fn with_scene_fm_palette_backend(self, backend: Arc<dyn MediaBackend>) -> Self {
        *self
            .scene_fm
            .palette
            .backend
            .lock()
            .expect("new palette backend") = Some(backend);
        self
    }
    fn scene_palette_source(
        &self,
        token: &SessionToken,
        expected: Option<u64>,
        voice: &str,
    ) -> Result<Source> {
        let state = SceneFmService::new(self.session.clone())?
            .cached_for(token)?
            .ok_or(Error::StaleResponse)?;
        // M2060 keys color by artwork, not queue/preparation revisions. A
        // same-voice auto-play synchronization may have advanced this revision
        // before IPC dispatch. Derive the current cover below; never reopen
        // artwork from the older observation or accept a future revision.
        if expected.is_some_and(|revision| revision > state.revision) {
            return Err(Error::StaleResponse);
        }
        let episode = state
            .selected()
            .filter(|v| v.id == voice)
            .ok_or(Error::StaleResponse)?;
        let cover = artwork::source(&episode.cover_url)?;
        Ok(Source {
            token: token.clone(),
            voice: voice.into(),
            cover,
        })
    }
    fn scene_palette_check(&self, source: &Source, sequence: u64) -> Result<()> {
        self.scene_fm.palette.current(sequence)?;
        if self.scene_palette_source(&source.token, None, &source.voice)? != *source {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
    fn scene_palette_artwork_check(&self, source: &ArtworkSource, sequence: u64) -> Result<()> {
        self.scene_fm.palette.current(sequence)?;
        let state = SceneFmService::new(self.session.clone())?
            .cached_for(&source.token)?
            .ok_or(Error::StaleResponse)?;
        let current = state.selected().ok_or(Error::StaleResponse)?;
        if artwork::source(&current.cover_url).ok().as_deref() != Some(source.cover.as_str()) {
            return Err(Error::StaleResponse);
        }
        // A new request must invalidate an older A even after A -> B -> A.
        self.scene_fm.palette.current(sequence)?;
        self.session.check_generation(&source.token)
    }
    pub async fn scene_fm_palette_read(
        &self,
        epoch: &str,
        expected_revision: &str,
        voice_id: &str,
        pixel_ratio: f64,
    ) -> Result<AccountData<SceneFmPaletteRead>> {
        let token = self.player_token(epoch)?;
        let size = artwork::thumbnail_size(pixel_ratio)?;
        let admitted =
            self.scene_palette_source(&token, Some(revision(expected_revision)?), voice_id)?;
        let mut superseded = self.scene_fm.palette.begin()?;
        let sequence = *superseded.borrow_and_update();
        // Keep the original exact current-voice admission; only its completed
        // admission is converted into the narrower public-artwork identity.
        self.scene_palette_check(&admitted, sequence)?;
        let source = ArtworkSource {
            token: admitted.token,
            cover: admitted.cover,
        };
        let cached = {
            let state = self
                .scene_fm
                .palette
                .state
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            state
                .cache
                .as_ref()
                .filter(|c| c.source == source)
                .map(|cache| cache.background)
        };
        if let Some(background) = cached {
            self.scene_palette_artwork_check(&source, sequence)?;
            return Ok(AccountData {
                epoch: epoch.into(),
                value: SceneFmPaletteRead::Ready { background },
            });
        }
        let backend = self.scene_fm.palette.backend()?;
        let mut generation = self.session.subscribe()?;
        let fetch = async {
            // Official NOS header response first. Failure, absent header, or
            // a thrown header parse alone proceeds to the separate image load.
            let decision = match artwork::header(backend.as_ref(), &source.cover, size).await {
                Ok(header) => {
                    if header.as_ref().is_some_and(|h| h.len() > 4096) {
                        return Ok::<_, Error>(SceneFmPaletteRead::Ready {
                            background: palette::background(None),
                        });
                    }
                    palette::parse_header(header.as_deref())
                }
                Err(_) => HeaderDecision::Sample,
            };
            self.scene_palette_artwork_check(&source, sequence)?;
            match decision {
                HeaderDecision::Parsed(colors) => Ok(SceneFmPaletteRead::Ready {
                    background: palette::background(colors.dominant),
                }),
                HeaderDecision::Sample => {
                    let image = artwork::image(backend.as_ref(), &source.cover, size).await?;
                    self.scene_palette_artwork_check(&source, sequence)?;
                    let receipt = uuid::Uuid::new_v4().simple().to_string();
                    let mut state = self
                        .scene_fm
                        .palette
                        .state
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?;
                    self.scene_fm.palette.current(sequence)?;
                    state.pending = Some(Pending {
                        source: source.clone(),
                        sequence,
                        receipt: receipt.clone(),
                        width: image.width,
                        height: image.height,
                    });
                    Ok(SceneFmPaletteRead::Sample {
                        receipt,
                        mime_type: image.mime.into(),
                        bytes: image.bytes,
                        width: image.width,
                        height: image.height,
                    })
                }
            }
        };
        let value = tokio::select! {
            biased;
            _ = generation.wait_for(|owner| *owner != token) => return Err(Error::StaleResponse),
            _ = superseded.changed() => return Err(Error::StaleResponse),
            result = tokio::time::timeout(LIMIT, fetch) => result.map_err(|_| Error::Transport("artwork color request timed out".into()))??,
        };
        self.scene_palette_artwork_check(&source, sequence)?;
        if let SceneFmPaletteRead::Ready { background } = &value {
            let mut state = self
                .scene_fm
                .palette
                .state
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            self.scene_fm.palette.current(sequence)?;
            state.cache = Some(Cache {
                source,
                background: *background,
            });
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub fn scene_fm_palette_sample(
        &self,
        epoch: &str,
        receipt: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<AccountData<SceneFmPaletteResult>> {
        let token = self.player_token(epoch)?;
        // Reject hostile renderer lengths before allocating any working image.
        if rgba.len() > 16_384 * 4
            || width == 0
            || height == 0
            || width > 50
            || height > 16_384
            || u64::from(width) * u64::from(height) * 4 != rgba.len() as u64
        {
            return Err(Error::InvalidInput("invalid artwork color sample size"));
        }
        let mut state = self
            .scene_fm
            .palette
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let pending = state
            .pending
            .as_ref()
            .filter(|p| p.receipt == receipt && p.source.token == token)
            .ok_or(Error::StaleResponse)?;
        let expected_width = pending.width.min(50);
        let expected_height = u64::from(pending.height)
            .checked_mul(u64::from(expected_width))
            .ok_or(Error::InvalidInput("invalid artwork color dimensions"))?
            .div_ceil(u64::from(pending.width));
        if width != expected_width || u64::from(height) != expected_height {
            return Err(Error::InvalidInput(
                "artwork color sample dimensions changed",
            ));
        }
        self.scene_palette_artwork_check(&pending.source, pending.sequence)?;
        let color = palette::sample_rgba(&rgba)
            .map_err(|_| Error::InvalidInput("invalid artwork color sample"))?;
        let background = palette::background(color);
        let source = pending.source.clone();
        state.pending = None;
        state.cache = Some(Cache { source, background });
        Ok(AccountData {
            epoch: epoch.into(),
            value: SceneFmPaletteResult { background },
        })
    }
    pub fn scene_fm_palette_cancel(&self, epoch: &str, receipt: &str) -> Result<()> {
        let token = self.player_token(epoch)?;
        let mut state = self
            .scene_fm
            .palette
            .state
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if state
            .pending
            .as_ref()
            .is_some_and(|p| p.source.token == token && p.receipt == receipt)
        {
            state.pending = None;
        }
        Ok(())
    }
}
