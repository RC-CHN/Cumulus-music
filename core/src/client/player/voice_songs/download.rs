//! Common guarded admission for ordinary and explicitly selected purchased songs.
use super::{
    MusicClient,
    actions::{Page, Row},
    id,
    purchased::{Offer, Selection, vivid_enabled},
};
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionToken,
        resources::{AccountResources, ResourceKind},
    },
    client::dto::{DownloadResult, SongDownloadState},
    download::service::DownloadOptions,
    membership::Quality,
    protocol::wire::serialization::js_truthy,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Class {
    Ordinary,
    Purchased(Offer),
    ExtendedRights,
    Cloud,
    Unavailable,
}

pub(super) enum Admission {
    Liked,
    Picker(Arc<AtomicBool>),
}

pub(super) fn classify(row: &Value, track_id: &str) -> Class {
    // M/21 A maps raw `dl`, `bd`, `cs`, `st` to maxDownBr,
    // maxPlayBd, cloudSong and status. M/27 and M/35 apply those fields.
    let p = &row["privilege"];
    if !p.is_object()
        || (!p["id"].is_null() && id(&p["id"]).as_deref() != Some(track_id))
        || ["programId", "audio", "radio", "mainTrackId"]
            .into_iter()
            .any(|key| js_truthy(&row[key]))
    {
        return Class::Unavailable;
    }
    if js_truthy(&p["cs"]) {
        return Class::Cloud;
    }
    if p["fee"].as_i64().is_some_and(|fee| fee >= 0)
        && let Some(offer) = Offer::from_privilege(p)
    {
        return Class::Purchased(offer);
    }
    if js_truthy(&p["payed"]) || p["fee"].as_i64().is_some_and(|fee| fee > 0) {
        return Class::ExtendedRights;
    }
    if p["fee"].as_i64() == Some(0)
        && p["st"].as_i64().is_some_and(|status| status >= 0)
        && p["dl"].as_u64().is_some_and(|bitrate| bitrate > 0)
        && !js_truthy(&p["bd"])
    {
        Class::Ordinary
    } else {
        Class::Unavailable
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSongDownloadDecision {
    Ready,
    PurchasedQualityRequired,
    PlaylistRequired,
    MembershipUnknown,
    Complete,
    ExistingTask,
    Waiting,
    Working,
    Unavailable,
    ExtendedRightsUnsupported,
    CloudUnsupported,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSongDownloadPlan {
    pub epoch: String,
    pub play_id: String,
    pub track_id: String,
    pub decision: VoiceSongDownloadDecision,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub purchased_qualities: Vec<Quality>,
}

#[derive(Clone, Copy)]
pub(super) struct Target<'a> {
    pub(super) epoch: &'a str,
    play_id: &'a str,
    card_receipt: &'a str,
    pub(super) index: usize,
    selected: Option<&'a str>,
}

impl<'a> Target<'a> {
    pub(super) fn ordinary(
        epoch: &'a str,
        play_id: &'a str,
        card_receipt: &'a str,
        index: usize,
    ) -> Self {
        Self {
            epoch,
            play_id,
            card_receipt,
            index,
            selected: None,
        }
    }

    pub(super) fn purchased<'b>(self, selected: &'b str) -> Target<'b>
    where
        'a: 'b,
    {
        Target {
            selected: Some(selected),
            ..self
        }
    }
}

pub(super) struct Bound {
    pub(super) page: Arc<Page>,
    pub(super) index: usize,
    selected: Option<Arc<Selection>>,
}

impl Bound {
    pub(super) fn row(&self) -> &Row {
        &self.page.rows[self.index]
    }

    pub(super) fn purchase_receipt(&self) -> Option<&str> {
        self.selected
            .as_ref()
            .map(|selection| selection.receipt.as_str())
    }
}

fn membership(
    session: Arc<AccountSession>,
    token: &SessionToken,
    id: &str,
) -> Result<VoiceSongDownloadDecision> {
    let (liked, pending) =
        AccountResources::new(session)?.membership_for(token, ResourceKind::Liked, id)?;
    Ok(
        match liked.filter(|_| !pending.is_some_and(|pending| pending.phase.pending())) {
            Some(true) => VoiceSongDownloadDecision::Ready,
            Some(false) => VoiceSongDownloadDecision::PlaylistRequired,
            None => VoiceSongDownloadDecision::MembershipUnknown,
        },
    )
}

impl MusicClient {
    pub(super) fn voice_song_download_scope(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
    ) -> Result<(
        Arc<Page>,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        self.voice_song_write_scope(epoch, play_id, receipt, |api| {
            api.song_card_download_scope_revision()
        })
    }

    pub(super) fn voice_song_bound_download(
        &self,
        target: Target<'_>,
    ) -> Result<(
        Bound,
        impl Fn() -> Result<()> + Send + Sync + 'static + use<>,
    )> {
        let (page, scope) =
            self.voice_song_download_scope(target.epoch, target.play_id, target.card_receipt)?;
        let row = page
            .rows
            .get(target.index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        let selected = match (row.download, target.selected) {
            (Class::Ordinary, None) => None,
            (Class::Purchased(_), Some(receipt)) => Some(page.purchase.get(target.index, receipt)?),
            _ => {
                return Err(Error::InvalidInput(
                    "invalid vinyl episode download row or purchase selection",
                ));
            }
        };
        let active = selected.clone();
        let experiments = self.pc_experiments.clone();
        let owner = page.clone();
        let check = move || {
            scope()?;
            if let Some(selection) = &active {
                selection.check()?;
                if selection.quality == Quality::Vivid && !vivid_enabled(&experiments, &owner)? {
                    return Err(Error::StaleResponse);
                }
            }
            Ok(())
        };
        check()?;
        Ok((
            Bound {
                page,
                index: target.index,
                selected,
            },
            check,
        ))
    }

    pub async fn player_voice_song_download_prepare(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
    ) -> Result<VoiceSongDownloadPlan> {
        self.voice_song_download_prepare(epoch, play_id, receipt, index, false)
            .await
    }

    pub(super) async fn voice_song_download_prepare(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
        show_purchase: bool,
    ) -> Result<VoiceSongDownloadPlan> {
        use VoiceSongDownloadDecision as D;
        let (page, check) = self.voice_song_download_scope(epoch, play_id, receipt)?;
        let row = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        let mut decision = self
            .voice_song_download_decision(epoch, &page, row, false)
            .await?;
        let mut purchased_qualities = Vec::new();
        if let (D::PurchasedQualityRequired, Class::Purchased(offer)) = (decision, row.download) {
            if show_purchase {
                purchased_qualities = offer.qualities(vivid_enabled(&self.pc_experiments, &page)?);
                if purchased_qualities.is_empty() {
                    decision = D::Unavailable;
                }
            } else {
                // Existing desktop IPC will not see a new decision until it
                // can complete the dedicated quality choice end to end.
                decision = D::ExtendedRightsUnsupported;
            }
        }
        check()?;
        Ok(VoiceSongDownloadPlan {
            epoch: epoch.into(),
            play_id: play_id.into(),
            track_id: row.track.id.clone(),
            decision,
            purchased_qualities,
        })
    }

    pub(super) async fn voice_song_download_decision(
        &self,
        epoch: &str,
        page: &Page,
        row: &Row,
        quality_selected: bool,
    ) -> Result<VoiceSongDownloadDecision> {
        use VoiceSongDownloadDecision as D;
        let song = row.track.id.clone();
        Ok(match self.song_download_state(epoch, &song).await? {
            SongDownloadState::Complete => D::Complete,
            SongDownloadState::Waiting => D::Waiting,
            SongDownloadState::Working => D::Working,
            SongDownloadState::Paused | SongDownloadState::Failed => D::ExistingTask,
            SongDownloadState::None => {
                let local = self.local.context().await?;
                let session = self.session.clone();
                let token = page.token.clone();
                let class = row.download;
                tokio::task::spawn_blocking(move || {
                    if !local
                        .library
                        .files_for_online_song(&session, &token, &song)?
                        .is_empty()
                    {
                        return Ok(D::Complete);
                    }
                    match class {
                        Class::Purchased(_) if !quality_selected => Ok(D::PurchasedQualityRequired),
                        Class::Ordinary | Class::Purchased(_) => membership(session, &token, &song),
                        Class::Cloud => Ok(D::CloudUnsupported),
                        Class::ExtendedRights => Ok(D::ExtendedRightsUnsupported),
                        Class::Unavailable => Ok(D::Unavailable),
                    }
                })
                .await
                .map_err(|_| Error::State("SongCard download admission worker stopped"))??
            }
        })
    }

    /// Ordinary direct branch for a song already in the liked library.
    pub async fn player_voice_song_download_start(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
        options: DownloadOptions,
    ) -> Result<DownloadResult> {
        let (page, check) = self.voice_song_download_scope(epoch, play_id, receipt)?;
        let row = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        if row.download != Class::Ordinary {
            return Err(Error::State(
                "song requires a different download authorization flow",
            ));
        }
        let bound = Bound {
            page,
            index,
            selected: None,
        };
        self.start_resolved_song_download(epoch, &bound, options, Admission::Liked, check)
            .await
    }

    pub(super) async fn start_resolved_song_download(
        &self,
        epoch: &str,
        bound: &Bound,
        options: DownloadOptions,
        admission: Admission,
        check: impl Fn() -> Result<()> + Send + 'static,
    ) -> Result<DownloadResult> {
        if bound
            .selected
            .as_ref()
            .is_some_and(|selection| options.quality != selection.quality)
        {
            return Err(Error::InvalidInput(
                "download options differ from the selected purchased quality",
            ));
        }
        let selected = bound.selected.clone();
        let id = bound.row().track.id.clone();
        let song = id.clone();
        let local = self.local.context().await?;
        let session = self.session.clone();
        let token = bound.page.token.clone();
        let (needs_liked, consumed) = match admission {
            Admission::Liked => (true, None),
            Admission::Picker(consumed) => (false, Some(consumed)),
        };
        let live_permit = consumed.clone();
        self.start_guarded_song_download(epoch, id, options, move || {
            if !local.library.files_for_online_song(&session, &token, &song)?.is_empty() {
                return Err(Error::State("song already has a local file"));
            }
            if needs_liked && membership(session.clone(), &token, &song)? != VoiceSongDownloadDecision::Ready {
                return Err(Error::State("SongCard download requires confirmed liked membership or an explicit playlist destination"));
            }
            if live_permit.as_ref().is_some_and(|permit| permit.load(Ordering::Acquire)) {
                return Err(Error::StaleResponse);
            }
            // Last after local cache work, immediately before durable admission.
            check()
        }, move || {
            if let Some(consumed) = consumed {
                consumed.store(true, Ordering::Release);
            }
            if let Some(selected) = selected {
                selected.retired.store(true, Ordering::Release);
            }
        }).await
    }
}
