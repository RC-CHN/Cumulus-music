//! Read-only "相关歌单", the independent sibling of the vinyl similar-song card.
use super::{
    MusicClient, VinylRecommendationPlacement,
    vinyl_similar::{observed_placement, seed, stale_credentials},
};
use crate::{
    Error, Result,
    protocol::{
        api::{VinylRelatedPlaylistsRead, identifier, validate_response},
        artwork,
    },
};
use serde::Serialize;
use serde_json::Value;

const MAX_ROWS: usize = 100;
const MAX_SAFE_JS_INTEGER: u64 = (1 << 53) - 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylRelatedPlaylistView {
    pub id: String,
    pub name: String,
    pub cover_url: Option<String>,
    pub play_count: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylRelatedPlaylistsView {
    pub play_id: String,
    pub song_id: String,
    pub placement: VinylRecommendationPlacement,
    pub playlists: Vec<VinylRelatedPlaylistView>,
}

fn projection(value: &Value) -> Result<Vec<VinylRelatedPlaylistView>> {
    let playlists = value["playlists"]
        .as_array()
        .filter(|playlists| playlists.len() <= MAX_ROWS)
        .ok_or(Error::Response("invalid vinyl related playlists"))?;
    playlists
        .iter()
        .map(|playlist| {
            let id = identifier(&playlist["id"])?;
            if id.len() > 32 {
                return Err(Error::Response("invalid vinyl related-playlist ID"));
            }
            let name = [&playlist["name"], &playlist["title"]]
                .into_iter()
                .filter_map(Value::as_str)
                .find(|value| !value.is_empty())
                .unwrap_or_default();
            // M/21.G's formatter chooses the first populated field. The
            // public artwork helper accepts only NetEase artwork hosts and
            // strips incoming tokens; no external URL is a navigation target.
            let cover_url = ["coverImgUrl", "picUrl", "coverUrl"]
                .into_iter()
                .map(|key| &playlist[key])
                .find(|value| value.as_str().is_some_and(|url| !url.is_empty()))
                .and_then(artwork::square);
            let play_count = playlist["playCount"]
                .as_u64()
                .or_else(|| playlist["playCount"].as_str()?.parse().ok())
                .filter(|count| *count <= MAX_SAFE_JS_INTEGER)
                .unwrap_or_default();
            Ok(VinylRelatedPlaylistView {
                id,
                name: name.into(),
                cover_url,
                play_count,
            })
        })
        .collect()
}

impl MusicClient {
    pub async fn player_vinyl_related_playlists(
        &self,
        epoch: &str,
        play_id: &str,
    ) -> Result<VinylRelatedPlaylistsView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let song_id = seed(&controller.snapshot(), &token, play_id)?;
        let read = VinylRelatedPlaylistsRead::new(&song_id)?;
        let (revision, placement) = self.vinyl_placement_for(&token, &api).await?;
        if !placement.resolved() {
            return Err(Error::State(
                "vinyl related playlists experiment is unavailable",
            ));
        }
        let check = || {
            self.session.require_api(&token, &api)?;
            if self.login_pending()?
                || api
                    .verified_pc_experiment_revision()
                    .map_err(stale_credentials)?
                    .as_deref()
                    != Some(revision.as_str())
                || observed_placement(&self.pc_experiments, &token, &api, &revision)? != placement
                || seed(&controller.snapshot(), &token, play_id)? != song_id
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check()?;
        let reply = api
            .request_vinyl_related_playlists(read, &revision)
            .await
            .map_err(stale_credentials)?;
        check()?;
        let value = validate_response(VinylRelatedPlaylistsRead::PATH, reply, &[200])?;
        let playlists = projection(&value)?;
        check()?;
        Ok(VinylRelatedPlaylistsView {
            play_id: play_id.into(),
            song_id,
            placement,
            playlists,
        })
    }
}
