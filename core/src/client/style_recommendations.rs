use super::{
    MusicClient,
    dto::{AccountData, TrackPage},
    mapping,
};
use crate::{
    Error, Result, account::SessionToken, catalog::CatalogApi, playback::engine::Cancellation,
};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct StyleRecommendations {
    started: u64,
    latest: Option<Snapshot>,
}

struct Snapshot {
    token: SessionToken,
    revision: String,
    ids: Vec<Value>,
}

impl MusicClient {
    pub async fn style_recommendations(&self, epoch: &str) -> Result<AccountData<TrackPage>> {
        let token = self.player_token(epoch)?;
        let request = self.session.with_token(&token, |_| {
            let mut state = self
                .style_recommendations
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            state.started = state
                .started
                .checked_add(1)
                .ok_or(Error::State("style recommendation revision exhausted"))?;
            Ok(state.started)
        })?;
        let mut value = self
            .session
            .run_for(&token, |api, _| async move {
                const MAX_TRACKS: usize = 500;
                let response = CatalogApi::new(api).default_style_recommendations().await?;
                let data = response["data"]
                    .as_object()
                    .ok_or(Error::Response("invalid style recommendations"))?;
                let rows = match data.get("dailySongs") {
                    None | Some(Value::Null) => &[][..],
                    Some(Value::Array(rows)) if rows.len() <= MAX_TRACKS => rows.as_slice(),
                    Some(_) => return Err(Error::Response("invalid style recommendation tracks")),
                };
                let mut ids = std::collections::HashSet::new();
                let tracks = rows
                    .iter()
                    .map(|row| {
                        let track = mapping::track(row)?;
                        if !ids.insert(track.id.clone()) {
                            return Err(Error::Response("duplicate style recommendation track"));
                        }
                        Ok(track)
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(TrackPage {
                    revision: None,
                    total: tracks.len(),
                    tracks,
                    offset: 0,
                    more: false,
                    playlist: None,
                })
            })
            .await?;
        self.session.with_token(&token, |_| {
            let mut state = self
                .style_recommendations
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.started != request {
                return Err(Error::StaleResponse);
            }
            let revision = request.to_string();
            state.latest = Some(Snapshot {
                token: token.clone(),
                revision: revision.clone(),
                ids: value.tracks.iter().map(|row| json!(row.id)).collect(),
            });
            value.revision = Some(revision);
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub(super) fn style_selection(
        &self,
        token: &SessionToken,
        revision: &str,
        index: usize,
    ) -> Result<(Vec<Value>, Cancellation)> {
        let ids = self.session.with_token(token, |_| {
            let state = self
                .style_recommendations
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let snapshot = state
                .latest
                .as_ref()
                .filter(|snapshot| snapshot.token == *token && snapshot.revision == revision)
                .ok_or(Error::QueueChanged)?;
            if index >= snapshot.ids.len() {
                return Err(Error::InvalidInput(
                    "invalid style recommendation selection index",
                ));
            }
            Ok(snapshot.ids.clone())
        })?;
        let state = self.style_recommendations.clone();
        let token = token.clone();
        let revision = revision.to_owned();
        let guard = Cancellation::new(move || {
            state.lock().is_ok_and(|state| {
                state.latest.as_ref().is_some_and(|snapshot| {
                    snapshot.token == token && snapshot.revision == revision
                })
            })
        });
        Ok((ids, guard))
    }
}
