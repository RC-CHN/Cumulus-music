//! Native prerequisite for the ordinary, already-liked SongCard download.
use super::{MusicClient, actions::Page, id};
use crate::{
    Error, Result,
    account::{
        AccountSession, SessionToken,
        resources::{AccountResources, ResourceKind},
    },
    client::dto::{DownloadResult, SongDownloadState},
    download::service::DownloadOptions,
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

    pub async fn player_voice_song_download_prepare(
        &self,
        epoch: &str,
        play_id: &str,
        receipt: &str,
        index: usize,
    ) -> Result<VoiceSongDownloadPlan> {
        use VoiceSongDownloadDecision as D;
        let (page, check) = self.voice_song_download_scope(epoch, play_id, receipt)?;
        let row = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        let id = row.track.id.clone();
        let decision = match self.song_download_state(epoch, &id).await? {
            SongDownloadState::Complete => D::Complete,
            SongDownloadState::Waiting => D::Waiting,
            SongDownloadState::Working => D::Working,
            SongDownloadState::Paused | SongDownloadState::Failed => D::ExistingTask,
            SongDownloadState::None => {
                let local = self.local.context().await?;
                let session = self.session.clone();
                let token = page.token.clone();
                let song = id.clone();
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
                        Class::Ordinary => membership(session, &token, &song),
                        Class::Cloud => Ok(D::CloudUnsupported),
                        Class::ExtendedRights => Ok(D::ExtendedRightsUnsupported),
                        Class::Unavailable => Ok(D::Unavailable),
                    }
                })
                .await
                .map_err(|_| Error::State("SongCard download admission worker stopped"))??
            }
        };
        check()?;
        Ok(VoiceSongDownloadPlan {
            epoch: epoch.into(),
            play_id: play_id.into(),
            track_id: id,
            decision,
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
        self.start_resolved_song_download(epoch, &page, index, options, Admission::Liked, check)
            .await
    }

    pub(super) async fn start_resolved_song_download(
        &self,
        epoch: &str,
        page: &Page,
        index: usize,
        options: DownloadOptions,
        admission: Admission,
        check: impl Fn() -> Result<()> + Send + 'static,
    ) -> Result<DownloadResult> {
        let row = page
            .rows
            .get(index)
            .ok_or(Error::InvalidInput("invalid vinyl episode song row"))?;
        if row.download != Class::Ordinary {
            return Err(Error::State(
                "song requires a different download authorization flow",
            ));
        }
        let id = row.track.id.clone();
        let song = id.clone();
        let local = self.local.context().await?;
        let session = self.session.clone();
        let token = page.token.clone();
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
        }).await
    }
}
