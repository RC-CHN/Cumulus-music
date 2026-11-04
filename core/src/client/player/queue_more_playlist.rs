//! Match a clearing playlist selection to the owner's already-read membership evidence.
use crate::{
    Error, Result,
    account::SessionToken,
    playback::queue_more::{PLAYLIST_MEMBERSHIP, PlaylistMembership},
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex};

use super::MusicClient;
use PlaylistMembership::{Created, Liked, Other, Subscribed, Unknown};

#[derive(Default)]
pub(super) struct MembershipCache(Mutex<Option<(SessionToken, Memberships)>>);

type Memberships = HashMap<String, PlaylistMembership>;

impl MembershipCache {
    fn replace(&self, token: &SessionToken, memberships: Memberships) -> Result<()> {
        *self.0.lock().map_err(|_| Error::LockPoisoned)? = Some((token.clone(), memberships));
        Ok(())
    }

    fn get(&self, token: &SessionToken, id: &str) -> Result<Option<PlaylistMembership>> {
        let cache = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(cache
            .as_ref()
            .filter(|(owner, _)| owner == token)
            .and_then(|(_, memberships)| memberships.get(id).copied()))
    }
}

impl MusicClient {
    pub(in crate::client) fn remember_queue_more_memberships(
        &self,
        token: &SessionToken,
        rows: &[Value],
    ) -> Result<()> {
        if !self.player.queue_more_enabled {
            return Ok(());
        }
        let uid = token
            .uid
            .as_deref()
            .ok_or(Error::State("playlist memberships require an account"))?;
        let memberships = host_memberships(uid, rows);
        self.session.with_token(token, |_| {
            self.player
                .queue_more_memberships
                .replace(token, memberships)
        })
    }

    pub(super) fn attach_queue_more_playlist(
        &self,
        token: &SessionToken,
        uid: &str,
        id: &str,
        detail: &Value,
        source: &mut Value,
    ) -> Result<()> {
        if !self.player.queue_more_enabled {
            return Ok(());
        }
        let membership = self.player.queue_more_memberships.get(token, id)?;
        let liked = self.intelligence_liked_hint(token)?;
        if let Some(membership) = classify(detail, id, uid, membership, liked.as_deref()) {
            source[PLAYLIST_MEMBERSHIP] = json!(membership);
        }
        Ok(())
    }
}

fn owner(row: &Value) -> std::result::Result<Option<String>, ()> {
    let direct = identifier(&row["userId"]).ok();
    let creator = identifier(&row["creator"]["userId"]).ok();
    if let (Some(direct), Some(creator)) = (&direct, &creator)
        && direct != creator
    {
        return Err(());
    }
    Ok(direct.or(creator))
}

fn host_memberships(uid: &str, rows: &[Value]) -> Memberships {
    let mut memberships = HashMap::new();
    for row in rows {
        let Ok(id) = identifier(&row["id"]) else {
            continue;
        };
        let candidate = match owner(row) {
            Ok(Some(owner)) if owner == uid => {
                if row["specialType"].as_i64() == Some(5) {
                    Liked
                } else {
                    Created
                }
            }
            // This response only lists the current viewer's created and
            // collected playlists. Missing rows provide no negative evidence.
            Ok(Some(_)) if row["subscribed"].as_bool() != Some(false) => Subscribed,
            _ => Unknown,
        };
        memberships
            .entry(id)
            .and_modify(|prior| {
                if *prior != candidate {
                    *prior = Unknown;
                }
            })
            .or_insert(candidate);
    }
    memberships
}

fn classify(
    detail: &Value,
    id: &str,
    uid: &str,
    known: Option<PlaylistMembership>,
    liked_id: Option<&str>,
) -> Option<PlaylistMembership> {
    if identifier(&detail["id"]).ok().as_deref() != Some(id) {
        return None;
    }
    let Ok(owner) = owner(detail) else {
        return Some(Unknown);
    };
    let liked = liked_id == Some(id) || known == Some(Liked);
    let own = match owner.as_deref() {
        Some(owner) => owner == uid,
        None => liked || known == Some(Created),
    };
    if own {
        return Some(match detail["specialType"].as_i64() {
            Some(5) => Liked,
            None if liked => Liked,
            _ => Created,
        });
    }
    let subscribed = detail["subscribed"].as_bool();
    Some(match (owner, subscribed) {
        (Some(_), Some(true)) => Subscribed,
        (Some(_), Some(false)) => Other,
        (_, None) if detail["subscribed"].is_null() && known == Some(Subscribed) => Subscribed,
        (None, Some(true)) if known == Some(Subscribed) => Subscribed,
        _ => Unknown,
    })
}
