use super::*;
use std::collections::HashSet;
pub(super) fn matches(record: &mut PlaylistMutation, observed: &Value, uid: &str) -> Result<bool> {
    match &record.command {
        PlaylistCommand::Create { name, private } => {
            let before: PlaylistList = serde_json::from_value(record.before.clone())?;
            let after: PlaylistList = serde_json::from_value(observed.clone())?;
            let candidates = after
                .playlists
                .iter()
                .filter(|p| {
                    p.owner == uid
                        && p.name == *name
                        && p.private == *private
                        && !p.favorite
                        && !before.playlists.iter().any(|old| old.id == p.id)
                        && record.playlist_id.as_ref().is_none_or(|id| id == &p.id)
                })
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                return Ok(false);
            }
            record.playlist_id = Some(candidates[0].id.clone());
            Ok(true)
        }
        PlaylistCommand::Order { group, ids, .. } => {
            let after: PlaylistList = serde_json::from_value(observed.clone())?;
            Ok(after.group(uid, *group) == *ids)
        }
        PlaylistCommand::Delete { id, .. }
        | PlaylistCommand::Subscribe {
            id,
            subscribed: false,
        } => {
            // `before` for deletion is a detail, not a list.
            if observed.get("playlists").is_none() {
                return Ok(false);
            }
            let after: PlaylistList = serde_json::from_value(observed.clone())?;
            Ok(!after.playlists.iter().any(|p| p.id == *id))
        }
        PlaylistCommand::Subscribe {
            id,
            subscribed: true,
        } => {
            let after: PlaylistList = serde_json::from_value(observed.clone())?;
            Ok(after
                .playlists
                .iter()
                .any(|p| p.id == *id && p.owner != uid))
        }
        command => {
            let after: PlaylistSnapshot = serde_json::from_value(observed.clone())?;
            after.writable(uid)?;
            if Some(after.summary.id.as_str()) != command.playlist_id() {
                return Err(Error::Response("playlist readback identity mismatch"));
            }
            match command {
                PlaylistCommand::Metadata {
                    name,
                    tags,
                    description,
                    ..
                } => Ok(after.summary.name == *name
                    && after.tags == *tags
                    && after.description == *description),
                PlaylistCommand::Cover { document_id, .. } => {
                    Ok(after.cover_id.as_ref() == Some(document_id))
                }
                PlaylistCommand::Publish { .. } => Ok(!after.summary.private),
                PlaylistCommand::Tracks { change, ids, .. } => {
                    let before: PlaylistSnapshot = serde_json::from_value(record.before.clone())?;
                    match change {
                        TrackChange::Delete => Ok(after.tracks
                            == before
                                .tracks
                                .iter()
                                .filter(|id| !ids.contains(id))
                                .cloned()
                                .collect::<Vec<_>>()),
                        TrackChange::Reorder | TrackChange::LegacyReplace => {
                            Ok(after.tracks == *ids)
                        }
                        TrackChange::Clear => Ok(after.tracks.is_empty()),
                        TrackChange::Add => {
                            let old = before.tracks.iter().collect::<HashSet<_>>();
                            let expected = before
                                .tracks
                                .iter()
                                .chain(ids.iter())
                                .collect::<HashSet<_>>();
                            // The service determines insertion positions; prove
                            // exact membership, count and unchanged old order.
                            Ok(after.tracks.len() == expected.len()
                                && after.tracks.iter().collect::<HashSet<_>>() == expected
                                && after
                                    .tracks
                                    .iter()
                                    .filter(|id| old.contains(id))
                                    .eq(before.tracks.iter()))
                        }
                    }
                }
                _ => unreachable!("list actions handled above"),
            }
        }
    }
}
