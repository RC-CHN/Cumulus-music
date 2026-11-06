//! Ordinary search artist cover playback: M/3095 -> M/3085 -> M/404.
use super::{MusicClient, PlaybackCommand, mapping};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::{ArtistApi, ArtistPageRequest},
    membership::Quality,
    playback::engine::Cancellation,
    protocol::api::identifier,
};
use serde_json::{Value, json};
use std::collections::HashSet;

const EXPERIMENT: &str = "PH-PC-Singer-Top50";
const LIMIT: usize = 50;

impl MusicClient {
    pub(super) async fn search_artist_selection(
        &self,
        token: &SessionToken,
        id: &str,
        quality: Quality,
    ) -> Result<(PlaybackCommand, Cancellation)> {
        let id = identifier(&json!(id))?;
        if id.len() > 32 {
            return Err(Error::InvalidInput("invalid search artist"));
        }
        let api = self.session.native_api(token)?;
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let revision = api
            .verified_pc_experiment_revision()
            .map_err(stale_credentials)?;
        let hot_songs = if let Some(revision) = &revision {
            match self.verified_pc_experiment_groups_for(token, &api).await {
                Ok((assigned, groups)) if assigned == *revision => {
                    groups.group(EXPERIMENT) == Some("t1")
                }
                Ok(_) => return Err(Error::StaleResponse),
                Err(err) => match stale_credentials(err) {
                    err @ (Error::StaleResponse | Error::LoginDeferred) => return Err(err),
                    // Missing, malformed or temporarily unavailable signed
                    // assignments never promote the treatment branch.
                    _ => false,
                },
            }
        } else {
            false
        };
        let check = || {
            self.session.require_api(token, &api)?;
            if self.login_pending()?
                || api
                    .verified_pc_experiment_revision()
                    .map_err(stale_credentials)?
                    != revision
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;

        let artist = ArtistApi::new(api.clone());
        let artist_id = json!(id);
        let (songs, name) = if hot_songs {
            let detail = artist.detail(&artist_id).await.map_err(stale_credentials)?;
            check()?;
            if identifier(&detail["artist"]["id"])? != id {
                return Err(Error::Response("search artist identity mismatch"));
            }
            let songs = cover_rows(&detail["hotSongs"])?;
            let ids = songs
                .iter()
                .map(|song| identifier(&song["id"]))
                .collect::<Result<Vec<_>>>()?;
            // M/612 always asks for privileges, including an empty ID array,
            // without trialMode. Failure does not switch to the control list.
            let privileges = crate::catalog::CatalogApi::new(api.clone())
                .privileges(&ids, None)
                .await
                .map_err(stale_credentials)?;
            check()?;
            (
                crate::catalog::merge_song_privileges(songs, &privileges)?,
                super::origin::resource_name(&detail["artist"], &id).map(str::to_owned),
            )
        } else {
            let response = artist
                .songs(
                    &artist_id,
                    ArtistPageRequest {
                        offset: 0,
                        limit: LIMIT,
                    },
                )
                .await
                .map_err(stale_credentials)?;
            check()?;
            let name = match crate::catalog::details::CatalogDetails::new(self.session.clone()) {
                Ok(catalog) => super::origin::cached_artist_name(&catalog, token, &id)?,
                Err(_) => None,
            };
            (cover_rows(&response["songs"])?.clone(), name)
        };

        let mut seen = HashSet::new();
        let mut resources = Vec::new();
        for song in songs {
            let song_id = identifier(&song["id"])?;
            // M/308's ordinary online queue keeps eligible rows in the
            // server's order; the shared resolver still authorizes audio.
            if !mapping::online_play_all_eligible(&song) || !seen.insert(song_id.clone()) {
                continue;
            }
            let metadata = mapping::song_metadata(&song)?;
            resources.push(
                json!({"id":song_id,"level":quality.as_str(),"dt":metadata["dt"],
                "missing":false,"song":metadata}),
            );
            if resources.len() == LIMIT {
                break;
            }
        }
        if resources.is_empty() {
            return Err(Error::State("artist has no playable songs"));
        }

        let session = self.session.clone();
        let expected = token.clone();
        let guard = Cancellation::new(move || {
            session.require_api(&expected, &api).is_ok()
                && api
                    .verified_pc_experiment_revision()
                    .is_ok_and(|current| current == revision)
        });
        Ok((
            PlaybackCommand::Replace {
                resources,
                index: 0,
                source: super::origin::named(
                    json!({"kind":"artist","id":id,"scene":"artist"}),
                    name.as_deref(),
                ),
            },
            guard,
        ))
    }
}

fn cover_rows(value: &Value) -> Result<&Vec<Value>> {
    value
        .as_array()
        .filter(|rows| rows.len() <= LIMIT && rows.iter().all(Value::is_object))
        .ok_or(Error::Response("invalid search artist songs"))
}

fn stale_credentials(error: Error) -> Error {
    if matches!(error.root(), Error::CredentialChanged) {
        Error::StaleResponse
    } else {
        error
    }
}
