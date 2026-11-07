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
    fn validate(&self, id: &str, owner: &str) -> Result<()> {
        if self.playlist.summary.id != id
            || !self.playlist.summary.favorite
            || self.playlist.summary.owner != owner
            || self.playlist.has_local
        {
            return Err(Error::Response("system liked journal observation differs"));
        }
        Ok(())
    }

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

struct Scope<'a> {
    service: &'a PlaylistService,
    token: &'a SessionToken,
    owner: String,
    api: Arc<dyn Api>,
    revision: String,
    outer: Option<&'a PlaylistAddPermission<'a>>,
}

impl<'a> Scope<'a> {
    fn new(
        service: &'a PlaylistService,
        token: &'a SessionToken,
        outer: Option<&'a PlaylistAddPermission<'a>>,
    ) -> Result<Self> {
        let owner = service.owner(token)?;
        let api = service.session.native_api(token)?;
        let revision = api
            .song_card_system_liked_scope_revision()?
            .ok_or(Error::State("signed system liked playlist is unavailable"))?;
        let scope = Self {
            service,
            token,
            owner,
            api,
            revision,
            outer,
        };
        scope.check()?;
        Ok(scope)
    }

    fn check_revision(&self) -> Result<()> {
        if self.api.song_card_system_liked_scope_revision()?.as_deref()
            != Some(self.revision.as_str())
        {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }

    fn check(&self) -> Result<()> {
        self.service.session.require_api(self.token, &self.api)?;
        self.check_revision()?;
        if let Some(outer) = self.outer {
            outer()?;
        }
        Ok(())
    }

    async fn read(&self, read: Read, path: &'static str) -> Result<Value> {
        let check = || self.check();
        check()?;
        let revision = self.revision.as_str();
        let response = self
            .service
            .session
            .run_for(self.token, |api, _| async move {
                validate_response(
                    path,
                    api.request_song_card_system_liked(read, revision, &check)
                        .await?,
                    &[200],
                )
            })
            .await?;
        check()?;
        Ok(response)
    }

    async fn playlist(&self, expected: Option<&str>) -> Result<Option<PlaylistSnapshot>> {
        let mut star = None;
        let mut seen = HashSet::new();
        let mut complete = false;
        for page in 0..Read::MAX_PAGES {
            let reply = self.read(Read::list(&self.owner, page), Read::LIST).await?;
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
                    let id = owned_star(row, &self.owner)?;
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
        if !complete {
            return Err(Error::Response("system liked host list is incomplete"));
        }
        if expected.is_some_and(|expected| star.as_deref() != Some(expected)) {
            return Err(Error::StaleResponse);
        }
        let Some(id) = star else {
            return Ok(None);
        };
        let detail = self.read(Read::detail(&id), Read::DETAIL).await?;
        if owned_star(&detail["playlist"], &self.owner)? != id {
            return Err(Error::Response("system liked playlist identity mismatch"));
        }
        let playlist = PlaylistSnapshot::from_detail(&detail["playlist"], &id)?;
        if playlist.has_local || playlist.tracks.len() > 100_000 {
            return Err(Error::State(
                "system liked playlist has unsupported entries",
            ));
        }
        Ok(Some(playlist))
    }

    async fn observe(&self, id: &str) -> Result<Observation> {
        let playlist = self.playlist(Some(id)).await?.ok_or(Error::StaleResponse)?;
        let membership = self
            .read(Read::membership(&self.owner), Read::MEMBERSHIP)
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
}

impl PlaylistService {
    pub(crate) async fn system_liked_destination(
        &self,
        token: &SessionToken,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<Option<String>> {
        let scope = Scope::new(self, token, Some(check))?;
        Ok(scope
            .playlist(None)
            .await?
            .map(|playlist| playlist.summary.id))
    }

    pub(super) async fn observe_system_liked(
        &self,
        token: &SessionToken,
        expected: &str,
        check: &PlaylistAddPermission<'_>,
    ) -> Result<Observation> {
        Scope::new(self, token, Some(check))?
            .observe(expected)
            .await
    }

    pub(super) async fn check_system_liked(
        &self,
        token: &SessionToken,
        mut record: PlaylistMutation,
        guard: Option<&PlaylistAddPermission<'_>>,
    ) -> Result<PlaylistMutation> {
        let scope = Scope::new(self, token, guard)?;
        let PlaylistCommand::Tracks {
            id,
            change: TrackChange::Add,
            ids,
            ..
        } = &record.command
        else {
            return Err(Error::State("invalid system liked journal command"));
        };
        let id = id.clone();
        let observation = scope.observe(&id).await?;
        let observed = serde_json::to_value(&observation)?;
        record.phase = if matches(&record, &observed, &scope.owner)? {
            PlaylistPhase::Confirmed
        } else if observation.check_selected_membership(ids).is_err() {
            // A definitive negative callback still does not settle contrary
            // signed membership views. Keep a read-only recovery available.
            PlaylistPhase::Uncertain
        } else {
            operation::unmatched_phase(&record)
        };
        record.observed = Some(observed);
        record.checked_at = Some(now()?);
        scope.check()?;
        self.session.with_token(token, |owner| {
            scope.check_revision()?;
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
        obs.validate(id, uid)?;
    }
    let liked = after.liked_ids.iter().collect::<HashSet<_>>();
    Ok(ids.iter().all(|id| liked.contains(id))
        && reconcile::tracks_added(&before.playlist, &after.playlist, ids))
}

/// System-liked continuation is based on the exact checked star in both
/// signed sources, including a coherent readback after a definitive false.
pub(super) fn verify_download_readback(
    record: &PlaylistMutation,
    owner: &str,
    id: &str,
    songs: &[String],
) -> Result<()> {
    let before: Observation = serde_json::from_value(record.before.clone())?;
    before.validate(id, owner)?;
    before.check_selected_membership(songs)?;
    if record.phase == PlaylistPhase::Unchanged {
        let liked = before.liked_ids.iter().collect::<HashSet<_>>();
        if songs.iter().all(|id| liked.contains(id)) {
            return Ok(());
        }
        return Err(Error::StaleResponse);
    }
    let after: Observation =
        serde_json::from_value(record.observed.clone().ok_or(Error::StaleResponse)?)?;
    after.validate(id, owner)?;
    after.check_selected_membership(songs)?;
    if record.phase == PlaylistPhase::Confirmed
        && !reconcile::tracks_added(&before.playlist, &after.playlist, songs)
    {
        return Err(Error::StaleResponse);
    }
    Ok(())
}
