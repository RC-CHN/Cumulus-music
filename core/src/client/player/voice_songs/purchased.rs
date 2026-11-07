//! M/3085's already-purchased quality choice, isolated from ordinary downloads.
use super::{
    MusicClient,
    actions::Page,
    download::{Admission, Class, Target, VoiceSongDownloadDecision, VoiceSongDownloadPlan},
    picker::{VoiceSongDownloadCreation, VoiceSongDownloadPicker},
};
use crate::{
    Error, Result,
    client::{dto::DownloadResult, pc_experiments::PcExperimentHost},
    download::service::DownloadOptions,
    membership::Quality,
};
use serde::Serialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Offer {
    max_song_kbps: Option<u64>,
    flags: u32,
}

impl Offer {
    pub(super) fn from_privilege(p: &serde_json::Value) -> Option<Self> {
        // M/35.h is a purchase mask, not a truthiness test. Numeric wire
        // fields and unsigned bounds avoid admitting malformed service data.
        if p["payed"].as_u64().and_then(|n| u32::try_from(n).ok())? & 6 == 0
            || p["st"].as_i64()? < 0
            || p["dl"].as_u64()? == 0
            || crate::protocol::wire::serialization::js_truthy(&p["bd"])
        {
            return None;
        }
        Some(Self {
            // M/21.A keeps the old maxbr-derived maxSongBr even if
            // maxBrLevel is available. Missing maxbr only removes base rows.
            max_song_kbps: p["maxbr"].as_u64().map(|n| n.div_ceil(1000)),
            flags: u32::try_from(p["flag"].as_u64().unwrap_or(0)).ok()?,
        })
    }

    pub(super) fn qualities(self, vivid: bool) -> Vec<Quality> {
        use Quality::*;
        // M/40.c + M/23.v, then M/3051 filters by tagSelectionMeta and
        // sorts by bitrate descending. No VIP or trial adds a choice here.
        [
            (Vivid, vivid && self.flags & (1 << 21) != 0),
            (Sky, self.flags & (1 << 18) != 0),
            (JyMaster, self.flags & (1 << 16) != 0),
            (JyEffect, self.flags & (1 << 17) != 0),
            (HiRes, self.flags & (1 << 12) != 0),
            (Lossless, self.base(999)),
            (ExHigh, self.base(320)),
            (Standard, self.base(128)),
        ]
        .into_iter()
        .filter_map(|(quality, offered)| offered.then_some(quality))
        .collect()
    }

    fn base(self, kbps: u64) -> bool {
        self.max_song_kbps.is_some_and(|max| max >= kbps)
    }
}

pub(super) struct Selection {
    pub(super) index: usize,
    pub(super) quality: Quality,
    pub(super) receipt: String,
    pub(super) retired: AtomicBool,
}
impl Selection {
    pub(super) fn check(&self) -> Result<()> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Shelf(Mutex<Option<Arc<Selection>>>);
impl Shelf {
    fn select(
        &self,
        index: usize,
        quality: Quality,
        check: impl FnOnce() -> Result<()>,
    ) -> Result<String> {
        // Do not hold the shelf across a native account/actor guard.
        check()?;
        let mut current = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if let Some(old) = current.as_ref()
            && old.index == index
            && old.quality == quality
            && old.check().is_ok()
        {
            return Ok(old.receipt.clone());
        }
        if let Some(old) = current.take() {
            old.retired.store(true, Ordering::Release);
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        *current = Some(Arc::new(Selection {
            index,
            quality,
            receipt: receipt.clone(),
            retired: AtomicBool::new(false),
        }));
        Ok(receipt)
    }

    pub(super) fn get(&self, index: usize, receipt: &str) -> Result<Arc<Selection>> {
        let choice = self
            .0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|choice| choice.index == index && choice.receipt == receipt)
            .cloned()
            .ok_or(Error::StaleResponse)?;
        choice.check()?;
        Ok(choice)
    }

    fn cancel(&self, index: usize, receipt: &str) -> Result<()> {
        self.get(index, receipt)?
            .retired
            .store(true, Ordering::Release);
        Ok(())
    }
}

/// Native-only commands. The existing ordinary download commands never
/// receive or manufacture an already-purchased quality receipt.
pub enum VoiceSongPurchasedDownloadCommand {
    Prepare,
    Select {
        quality: Quality,
    },
    Cancel {
        selection: String,
    },
    ChoosePlaylist {
        selection: String,
        playlist_id: String,
    },
    PickerStatus {
        selection: String,
        reconcile: bool,
    },
    CreatePlaylist {
        selection: String,
        name: String,
        private: bool,
    },
    CreationStatus {
        selection: String,
        reconcile: bool,
    },
    AddCreatedPlaylist {
        selection: String,
    },
    Start {
        selection: String,
        continuation: Option<String>,
        options: DownloadOptions,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceSongPurchasedDownloadResult {
    Plan {
        plan: VoiceSongDownloadPlan,
    },
    Selection {
        quality: Quality,
        decision: VoiceSongDownloadDecision,
        selection: Option<String>,
    },
    Cancelled,
    Picker {
        picker: VoiceSongDownloadPicker,
    },
    Creation {
        creation: VoiceSongDownloadCreation,
    },
    DirectoryRequired,
    Started {
        result: DownloadResult,
    },
}

pub(super) fn vivid_enabled(experiments: &PcExperimentHost, page: &Page) -> Result<bool> {
    let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
    Ok(experiments
        .observed_groups(&page.token, &api, &page.revision)?
        .is_some_and(|groups| {
            groups.group("PH-PC-hyyinzhi") == Some("t1")
                && groups.group("PH-PC-zhenyingn") == Some("t1")
        }))
}

impl MusicClient {
    /// M/31 opens a fixed public M/40 help page in the system browser.
    /// The URL is selected natively from the current verified PC assignment.
    pub fn voice_song_purchased_quality_guide(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
    ) -> Result<&'static str> {
        let (page, check) = self.voice_song_download_scope(epoch, play_id, receipt)?;
        let row = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        let Class::Purchased(offer) = row.download else {
            return Err(Error::State("song has no purchased quality guide"));
        };
        let vivid = vivid_enabled(&self.pc_experiments, &page)?;
        if offer.qualities(vivid).is_empty() {
            return Err(Error::State("song has no available purchased quality"));
        }
        check()?;
        Ok(if vivid {
            "https://music.163.com/g/st-vip-audio-quality"
        } else {
            "https://y.music.163.com/m/at/648fba3cbe7c73d87cbc2e81"
        })
    }

    pub async fn player_voice_song_purchased_download(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
        command: VoiceSongPurchasedDownloadCommand,
    ) -> Result<VoiceSongPurchasedDownloadResult> {
        use VoiceSongPurchasedDownloadCommand as C;
        use VoiceSongPurchasedDownloadResult as R;
        let target = Target::ordinary(epoch, play_id, receipt, index);
        match command {
            C::Prepare => Ok(R::Plan {
                plan: self
                    .voice_song_download_prepare(epoch, play_id, receipt, index, true)
                    .await?,
            }),
            C::Select { quality } => {
                let (page, check) = self.voice_song_download_scope(epoch, play_id, receipt)?;
                let row = page
                    .rows
                    .get(index)
                    .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
                let Class::Purchased(offer) = row.download else {
                    return Err(Error::InvalidInput(
                        "song has no verified purchased download offer",
                    ));
                };
                if !offer
                    .qualities(vivid_enabled(&self.pc_experiments, &page)?)
                    .contains(&quality)
                {
                    return Err(Error::InvalidInput(
                        "quality is not offered for this purchase",
                    ));
                }
                let decision = self
                    .voice_song_download_decision(epoch, &page, row, true)
                    .await?;
                check()?;
                let selection = if matches!(
                    decision,
                    VoiceSongDownloadDecision::Ready | VoiceSongDownloadDecision::PlaylistRequired
                ) {
                    Some(page.purchase.select(index, quality, || {
                        check()?;
                        if quality == Quality::Vivid && !vivid_enabled(&self.pc_experiments, &page)?
                        {
                            return Err(Error::StaleResponse);
                        }
                        Ok(())
                    })?)
                } else {
                    None
                };
                Ok(R::Selection {
                    quality,
                    decision,
                    selection,
                })
            }
            C::Cancel { selection } => {
                let (page, check) = self.voice_song_download_scope(epoch, play_id, receipt)?;
                check()?;
                page.purchase.cancel(index, &selection)?;
                Ok(R::Cancelled)
            }
            C::ChoosePlaylist {
                selection,
                playlist_id,
            } => Ok(R::Picker {
                picker: self
                    .voice_song_choose_playlist(target.purchased(&selection), &playlist_id)
                    .await?,
            }),
            C::PickerStatus {
                selection,
                reconcile,
            } => Ok(R::Picker {
                picker: self
                    .voice_song_picker_status(target.purchased(&selection), reconcile)
                    .await?,
            }),
            C::CreatePlaylist {
                selection,
                name,
                private,
            } => Ok(R::Creation {
                creation: self
                    .voice_song_create_playlist(target.purchased(&selection), &name, private)
                    .await?,
            }),
            C::CreationStatus {
                selection,
                reconcile,
            } => Ok(R::Creation {
                creation: self
                    .voice_song_creation_status(target.purchased(&selection), reconcile)
                    .await?,
            }),
            C::AddCreatedPlaylist { selection } => Ok(R::Creation {
                creation: self
                    .voice_song_add_created_playlist(target.purchased(&selection))
                    .await?,
            }),
            C::Start {
                selection,
                continuation,
                options,
            } => {
                let target = target.purchased(&selection);
                let (bound, check) = self.voice_song_bound_download(target)?;
                let available = self.configured_download_directory_available(epoch).await?;
                check()?;
                if !available {
                    return Ok(R::DirectoryRequired);
                }
                let result = if let Some(continuation) = continuation {
                    self.voice_song_start_from_playlist(target, &continuation, options)
                        .await?
                } else {
                    self.start_resolved_song_download(
                        epoch,
                        &bound,
                        options,
                        Admission::Liked,
                        check,
                    )
                    .await?
                };
                Ok(R::Started { result })
            }
        }
    }
}
