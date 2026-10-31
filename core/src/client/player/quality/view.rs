use crate::{
    Result,
    membership::Quality,
    playback::{
        TimelineMode,
        controller::PlaybackSnapshot,
        quality::{
            QualityAccess, QualityAvailability, QualityExperiments, QualityInformation,
            QualityPolicy, QualityReadStatus, QualityRight, TrialPhase,
        },
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityChoiceView {
    pub quality: Quality,
    pub availability: QualityAvailability,
    pub access: QualityAccess,
    pub size_bytes: Option<String>,
    pub trial_phase: Option<TrialPhase>,
    pub remaining_ms: Option<u64>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityDetailsView {
    pub epoch: String,
    pub play_id: String,
    pub song_id: String,
    pub revision: String,
    pub choices: Vec<QualityChoiceView>,
    pub status: QualityReadStatus,
    pub refresh_after_ms: Option<u64>,
}
pub(super) fn project(
    epoch: &str,
    play_id: &str,
    song: &str,
    current: &PlaybackSnapshot,
    information: &QualityInformation,
) -> Result<QualityDetailsView> {
    let trial = current
        .timeline
        .as_ref()
        .is_some_and(|t| matches!(t.mode, TimelineMode::Trial | TimelineMode::FullTrial));
    let policy = QualityPolicy {
        privilege: information.privilege.as_ref(),
        subscriber: information.subscriber,
        use_rights: &information.use_rights,
        experiments: QualityExperiments::default(),
        trial,
    };
    let choices: Vec<_> = [
        Quality::Vivid,
        Quality::Sky,
        Quality::JyMaster,
        Quality::JyEffect,
        Quality::HiRes,
        Quality::Lossless,
        Quality::ExHigh,
        Quality::Standard,
        Quality::Higher,
        Quality::Dolby,
        Quality::Aac64,
    ]
    .into_iter()
    .map(|quality| {
        let use_right = QualityRight::for_quality(quality)
            .and_then(|r| information.use_rights.get(&r))
            .and_then(Option::as_ref);
        let phase = use_right.map(|r| r.phase());
        QualityChoiceView {
            quality,
            availability: policy.availability(quality),
            access: policy.access(quality),
            size_bytes: information
                .sizes
                .as_ref()
                .and_then(|s| s.bytes(quality))
                .map(|v| v.to_string()),
            trial_phase: phase,
            remaining_ms: use_right
                .filter(|_| matches!(phase, Some(TrialPhase::Eligible | TrialPhase::Active)))
                .and_then(|r| r.remaining_ms),
        }
    })
    .collect();
    // Countdown and byte-size updates do not invalidate consent. The offered
    // duration of an as-yet unstarted trial does: changing it requires review.
    let stable: Vec<_> = choices
        .iter()
        .map(|c| {
            (
                &c.quality,
                &c.availability,
                &c.access,
                &c.trial_phase,
                if c.trial_phase == Some(TrialPhase::Eligible) {
                    c.remaining_ms
                } else {
                    None
                },
            )
        })
        .collect();
    let revision = hex::encode(Sha256::digest(serde_json::to_vec(&(
        epoch,
        play_id,
        song,
        &current.quality_preferences.preferences.revision,
        stable,
    ))?));
    let active = choices
        .iter()
        .filter(|c| c.trial_phase == Some(TrialPhase::Active))
        .map(|c| c.remaining_ms.unwrap_or(15000).clamp(1000, i32::MAX as u64))
        .min();
    let membership = information
        .membership_refresh_after_ms
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| v.ceil().min(i32::MAX as f64) as u64);
    let refresh_after_ms = active.into_iter().chain(membership).min();
    Ok(QualityDetailsView {
        epoch: epoch.into(),
        play_id: play_id.into(),
        song_id: song.into(),
        revision,
        choices,
        status: information.status.clone(),
        refresh_after_ms,
    })
}
