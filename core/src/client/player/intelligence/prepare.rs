use super::*;
use crate::{
    catalog::{CatalogApi, intelligence::MAX_LIKED_TRACKS},
    playlist::PlaylistApi,
};
use sha2::{Digest, Sha256};

pub(super) struct Prepared {
    pub resources: Vec<Value>,
    pub metadata: Vec<Value>,
    pub count: usize,
}

impl MusicClient {
    pub(super) async fn intelligence_liked_id(
        &self,
        token: &SessionToken,
        flight: Option<&Flight>,
    ) -> Result<Option<String>> {
        let read = self.session.run_for(token, |api, uid| async move {
            let rows = PlaylistApi::new(api).list(&json!(uid)).await?;
            rows.iter()
                .find(|p| {
                    p["specialType"] == 5
                        && identifier(p.get("userId").unwrap_or(&p["creator"]["userId"]))
                            .ok()
                            .as_ref()
                            == Some(&uid)
                })
                .map(|p| identifier(&p["id"]))
                .transpose()
        });
        match flight {
            Some(f) => f.wait(&self.session, read).await,
            None => read.await,
        }
    }

    pub(super) async fn intelligence_catalog(
        &self,
        flight: &Flight,
        playlist: &str,
        revision: Option<&str>,
        intent: Option<u64>,
    ) -> Result<(Vec<Option<Value>>, usize)> {
        let token = &flight.token;
        let detail = flight
            .wait(
                &self.session,
                self.session.run_for(token, |api, _| async move {
                    PlaylistApi::new(api).detail(&json!(playlist)).await
                }),
            )
            .await?;
        let ids = detail["trackIds"]
            .as_array()
            .ok_or(Error::Response("missing authoritative trackIds"))?;
        if ids.is_empty() {
            return Err(Error::State("intelligence requires playable liked tracks"));
        }
        if ids.len() > MAX_LIKED_TRACKS {
            return Err(Error::Response("too many intelligence liked tracks"));
        }
        if let Some(revision) = revision {
            let actual = hex::encode(Sha256::digest(serde_json::to_vec(&json!([playlist, ids]))?));
            if revision != actual {
                return Err(Error::QueueChanged);
            }
        }
        // Keep holes and order: M/59.O stops at the first unresolved song.
        // Local-only (-1) IDs have no recovered portable catalogue resolution.
        let mut songs = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            if let Some(intent) = intent {
                self.intelligence_start_guard(flight, intent)?;
            }
            let online: Vec<_> = chunk
                .iter()
                .filter(|v| v["v"] != -1)
                .map(|v| v["id"].clone())
                .collect();
            let batch = flight
                .wait(
                    &self.session,
                    self.session.run_for(token, |api, _| async move {
                        CatalogApi::new(api).songs(&online, 500).await
                    }),
                )
                .await?;
            let mut found = batch.songs.into_iter();
            for item in chunk {
                let mut song = if item["v"] == -1 {
                    None
                } else {
                    found.next().flatten()
                };
                if let Some(song) = song.as_mut() {
                    let id = identifier(&song["id"])?;
                    if let Some(privilege) = batch.privileges.get(&id) {
                        song["privilege"] = privilege.clone();
                    }
                }
                songs.push(song);
            }
        }
        if let Some(intent) = intent {
            self.intelligence_start_guard(flight, intent)?;
        }
        Ok((songs, ids.len()))
    }

    pub(super) async fn intelligence_prepare(
        &self,
        flight: &Flight,
        playlist: &str,
        revision: Option<&str>,
        intent: u64,
    ) -> Result<Prepared> {
        let (songs, count) = self
            .intelligence_catalog(flight, playlist, revision, Some(intent))
            .await?;
        let metadata = songs.iter().flatten().cloned().collect();
        let resources = self.intelligence_filter(flight, songs).await?;
        self.intelligence_start_guard(flight, intent)?;
        if resources.is_empty() {
            return Err(Error::State("intelligence requires playable liked tracks"));
        }
        Ok(Prepared {
            resources,
            metadata,
            count,
        })
    }
}
