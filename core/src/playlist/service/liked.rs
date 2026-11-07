//! Native-only observation for the current account's system liked playlist.
use super::*;
use crate::{
    account::resources::ResourceKind,
    protocol::api::{SongCardLikedRead as Read, validate_response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Serialize, Deserialize)]
pub(super) struct Observation {
    pub(super) playlist: PlaylistSnapshot,
    liked_ids: Vec<String>,
}

impl Observation {
    pub(super) fn check_selected_membership(&self, songs: &[String]) -> Result<()> {
        let playlist = self.playlist.tracks.iter().collect::<HashSet<_>>();
        let liked = self.liked_ids.iter().collect::<HashSet<_>>();
        if songs
            .iter()
            .any(|song| playlist.contains(song) != liked.contains(song))
        {
            return Err(Error::Response("system liked preflight membership differs"));
        }
        Ok(())
    }
}

fn owned_star(row: &Value, uid: &str) -> Result<String> {
    let id = identifier(&row["id"])?;
    let direct = row
        .get("userId")
        .filter(|id| !id.is_null())
        .map(identifier)
        .transpose()?;
    let creator = row["creator"]
        .get("userId")
        .filter(|id| !id.is_null())
        .map(identifier)
        .transpose()?;
    if row["specialType"].as_i64() != Some(5)
        || direct.as_ref().or(creator.as_ref()).map(String::as_str) != Some(uid)
        || direct
            .as_ref()
            .zip(creator.as_ref())
            .is_some_and(|(a, b)| a != b)
    {
        return Err(Error::InvalidInput(
            "verified system liked playlist required",
        ));
    }
    Ok(id)
}

impl PlaylistService {
    async fn liked_read(
        &self,
        token: &SessionToken,
        read: Read,
        path: &'static str,
        revision: &str,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<Value> {
        check()?;
        let response = self
            .session
            .run_for(token, |api, _| async move {
                validate_response(
                    path,
                    api.request_song_card_system_liked(read, revision, check)
                        .await?,
                    &[200],
                )
            })
            .await?;
        check()?;
        Ok(response)
    }

    pub(super) async fn observe_system_liked(
        &self,
        token: &SessionToken,
        expected: Option<&str>,
        outer: Option<&PlaylistAddPermission<'_>>,
    ) -> Result<Observation> {
        let uid = self.owner(token)?;
        let api = self.session.native_api(token)?;
        let revision = api
            .song_card_system_liked_scope_revision()?
            .ok_or(Error::State("signed system liked playlist is unavailable"))?;
        let check = || {
            self.session.require_api(token, &api)?;
            if api.song_card_system_liked_scope_revision()?.as_deref() != Some(revision.as_str()) {
                return Err(Error::CredentialChanged);
            }
            if let Some(outer) = outer {
                outer()?;
            }
            Ok(())
        };
        let mut star = None;
        let mut seen = HashSet::new();
        let mut complete = false;
        for page in 0..Read::MAX_PAGES {
            let reply = self
                .liked_read(token, Read::list(&uid, page), Read::LIST, &revision, &check)
                .await?;
            let rows = reply["playlist"]
                .as_array()
                .ok_or(Error::Response("system liked host list missing"))?;
            let more = reply["more"]
                .as_bool()
                .ok_or(Error::Response("system liked host list is incomplete"))?;
            if rows.len() > Read::LIST_LIMIT || (more && rows.is_empty()) {
                return Err(Error::Response("invalid system liked host list page"));
            }
            for row in rows {
                let id = identifier(&row["id"])?;
                if !seen.insert(id) {
                    return Err(Error::Response("duplicate system liked host list ID"));
                }
                if row["specialType"].as_i64() == Some(5) {
                    let id = owned_star(row, &uid)?;
                    if star.replace(id).is_some() {
                        return Err(Error::Response("ambiguous system liked playlist"));
                    }
                }
            }
            if !more {
                complete = true;
                break;
            }
        }
        let id = star
            .filter(|id| complete && expected.is_none_or(|expected| expected == id))
            .ok_or(Error::State(
                "current system liked playlist could not be verified",
            ))?;
        let detail = self
            .liked_read(token, Read::detail(&id), Read::DETAIL, &revision, &check)
            .await?;
        if owned_star(&detail["playlist"], &uid)? != id {
            return Err(Error::Response("system liked playlist identity mismatch"));
        }
        let playlist = PlaylistSnapshot::from_detail(&detail["playlist"], &id)?;
        if playlist.has_local || playlist.tracks.len() > 100_000 {
            return Err(Error::State(
                "system liked playlist has unsupported entries",
            ));
        }
        let membership = self
            .liked_read(
                token,
                Read::membership(&uid),
                Read::MEMBERSHIP,
                &revision,
                &check,
            )
            .await?;
        let ids = membership["ids"]
            .as_array()
            .filter(|ids| ids.len() <= 100_000)
            .ok_or(Error::Response("missing bounded liked song membership"))?;
        let liked_ids = ids.iter().map(identifier).collect::<Result<Vec<_>>>()?;
        let distinct = liked_ids.iter().collect::<HashSet<_>>();
        if distinct.len() != liked_ids.len() {
            return Err(Error::Response("duplicate liked song membership"));
        }
        Ok(Observation {
            playlist,
            liked_ids,
        })
    }

    pub(super) async fn check_system_liked(
        &self,
        token: &SessionToken,
        mut record: PlaylistMutation,
        guard: Option<&PlaylistAddPermission<'_>>,
    ) -> Result<PlaylistMutation> {
        let uid = self.owner(token)?;
        let api = self.session.native_api(token)?;
        let revision = api
            .song_card_system_liked_scope_revision()?
            .ok_or(Error::State("signed system liked playlist is unavailable"))?;
        let revision_current = || {
            if api.song_card_system_liked_scope_revision()?.as_deref() != Some(revision.as_str()) {
                return Err(Error::CredentialChanged);
            }
            Ok(())
        };
        let current = || {
            self.session.require_api(token, &api)?;
            revision_current()?;
            if let Some(guard) = guard {
                guard()?;
            }
            Ok(())
        };
        let id = record
            .command
            .playlist_id()
            .ok_or(Error::State("system liked operation missing playlist"))?
            .to_owned();
        let observation = self
            .observe_system_liked(token, Some(&id), Some(&current))
            .await?;
        let observed = serde_json::to_value(&observation)?;
        record.phase = if matches(&record, &observed, &uid)? {
            PlaylistPhase::Confirmed
        } else {
            operation::unmatched_phase(&record)
        };
        record.observed = Some(observed);
        record.checked_at = Some(now()?);
        current()?;
        self.session.with_token(token, |owner| {
            revision_current()?;
            self.store.transaction(|tx| {
                tx.put(
                    owner,
                    JOURNAL,
                    &record.operation_id,
                    &serde_json::to_value(&record)?,
                )?;
                if record.phase == PlaylistPhase::Confirmed {
                    tx.put(
                        owner,
                        "playlist-cache",
                        &id,
                        &serde_json::to_value(&observation.playlist)?,
                    )?;
                    tx.put(
                        owner,
                        "resources",
                        ResourceKind::Liked.as_str(),
                        &json!(observation.liked_ids),
                    )?;
                }
                Ok(())
            })
        })?;
        Ok(record)
    }
}

pub(super) fn matches(record: &PlaylistMutation, observed: &Value, uid: &str) -> Result<bool> {
    let PlaylistCommand::Tracks {
        id,
        change: TrackChange::Add,
        ids,
        ..
    } = &record.command
    else {
        return Err(Error::State("invalid system liked journal command"));
    };
    if record.playlist_id.as_ref() != Some(id)
        || !(1..=GuardedPlaylistAdd::MAX_SONGS).contains(&ids.len())
    {
        return Err(Error::State("invalid system liked journal destination"));
    }
    let before: Observation = serde_json::from_value(record.before.clone())?;
    let after: Observation = serde_json::from_value(observed.clone())?;
    for obs in [&before, &after] {
        if obs.playlist.summary.id != *id
            || !obs.playlist.summary.favorite
            || obs.playlist.summary.owner != uid
            || obs.playlist.has_local
        {
            return Err(Error::Response("system liked journal observation differs"));
        }
    }
    let liked = after.liked_ids.iter().collect::<HashSet<_>>();
    Ok(ids.iter().all(|id| liked.contains(id))
        && reconcile::tracks_added(&before.playlist, &after.playlist, ids))
}
