//! Native playback identity shared by VoicePage detail reads.
use super::{PlaybackSnapshot, PlaybackStatus, PlayerResourceKind, identity, voice_detail};
use crate::{Error, Result, account::SessionToken};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct VoicePlaybackSeed {
    pub(super) voice_id: String,
    pub(super) song_id: String,
    pub(super) entry_id: String,
    source: EpisodeSource,
}

#[derive(Clone, PartialEq, Eq)]
enum EpisodeSource {
    Voice,
    Scene { id: String, revision: u64 },
    Download { task_id: String },
}

pub(super) fn seed(
    snapshot: &PlaybackSnapshot,
    token: &SessionToken,
    play_id: &str,
) -> Result<VoicePlaybackSeed> {
    if play_id.is_empty()
        || &snapshot.account != token
        || snapshot.play_id.as_deref() != Some(play_id)
        || snapshot.fm
        || !matches!(
            snapshot.state,
            PlaybackStatus::Playing | PlaybackStatus::Paused | PlaybackStatus::Buffering
        )
    {
        return Err(Error::StaleResponse);
    }
    let entry = snapshot.selected.as_ref().ok_or(Error::StaleResponse)?;
    // M/23 and M/308 classify ordinary/Scene FM voices and downloaded
    // voice origins. An ordinary indexed file is not an episode producer.
    let (voice_id, song_id, _) =
        voice_detail::bound_episode(&entry.resource).ok_or(Error::StaleResponse)?;
    let source = match identity::kind(&entry.resource) {
        PlayerResourceKind::Voice if entry.source["kind"] == "scene_fm" => {
            let (scene, voice, revision) = snapshot
                .committed_scene_voice(token)
                .ok_or(Error::StaleResponse)?;
            if voice != voice_id
                || entry.source["scene_id"].as_str() != Some(scene)
                || entry.source["scene_revision"].as_u64() != Some(revision)
            {
                return Err(Error::StaleResponse);
            }
            EpisodeSource::Scene {
                id: scene.into(),
                revision,
            }
        }
        PlayerResourceKind::Voice => EpisodeSource::Voice,
        PlayerResourceKind::Downloaded if entry.source["kind"] != "scene_fm" => {
            EpisodeSource::Download {
                task_id: entry.resource["download_task_id"]
                    .as_str()
                    .ok_or(Error::StaleResponse)?
                    .into(),
            }
        }
        _ => return Err(Error::StaleResponse),
    };
    Ok(VoicePlaybackSeed {
        voice_id,
        song_id,
        entry_id: entry.entry_id.clone(),
        source,
    })
}
