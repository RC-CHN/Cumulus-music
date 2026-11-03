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
pub(super) struct RecommendationSnapshot {
    started: u64,
    latest: Option<Snapshot>,
}

struct Snapshot {
    token: SessionToken,
    revision: String,
    ids: Vec<Value>,
}

#[derive(Clone, Copy)]
pub(super) enum RecommendationSource {
    Daily,
    Style,
}

impl MusicClient {
    fn recommendation_state(
        &self,
        source: RecommendationSource,
    ) -> &std::sync::Arc<std::sync::Mutex<RecommendationSnapshot>> {
        match source {
            RecommendationSource::Daily => &self.daily_recommendations,
            RecommendationSource::Style => &self.style_recommendations,
        }
    }

    fn begin_recommendation(&self, source: RecommendationSource) -> Result<u64> {
        let mut state = self
            .recommendation_state(source)
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        state.started = state
            .started
            .checked_add(1)
            .ok_or(Error::State("recommendation revision exhausted"))?;
        Ok(state.started)
    }

    pub async fn daily_songs(&self) -> Result<AccountData<TrackPage>> {
        let source = RecommendationSource::Daily;
        let request = self.begin_recommendation(source)?;
        let (token, value) = self
            .session
            .run(|api, _| async move {
                let response = CatalogApi::new(api).daily().await?;
                recommendation_tracks(&response, source)
            })
            .await?;
        self.publish_recommendation(source, request, token, value)
    }

    pub async fn style_recommendations(&self, epoch: &str) -> Result<AccountData<TrackPage>> {
        let token = self.player_token(epoch)?;
        let source = RecommendationSource::Style;
        let request = self
            .session
            .with_token(&token, |_| self.begin_recommendation(source))?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = CatalogApi::new(api).default_style_recommendations().await?;
                recommendation_tracks(&response, source)
            })
            .await?;
        self.publish_recommendation(source, request, token, value)
    }

    fn publish_recommendation(
        &self,
        source: RecommendationSource,
        request: u64,
        token: SessionToken,
        mut value: TrackPage,
    ) -> Result<AccountData<TrackPage>> {
        self.session.with_token(&token, |_| {
            let mut state = self
                .recommendation_state(source)
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.started != request {
                return Err(Error::StaleResponse);
            }
            let revision = match source {
                RecommendationSource::Daily => format!("daily-{request}"),
                RecommendationSource::Style => format!("style-{request}"),
            };
            state.latest = Some(Snapshot {
                token: token.clone(),
                revision: revision.clone(),
                ids: value.tracks.iter().map(|row| json!(row.id)).collect(),
            });
            value.revision = Some(revision);
            Ok(AccountData {
                epoch: token.generation.to_string(),
                value,
            })
        })
    }

    pub(super) fn recommendation_selection(
        &self,
        source: RecommendationSource,
        token: &SessionToken,
        revision: &str,
        index: usize,
    ) -> Result<(Vec<Value>, Cancellation)> {
        let ids = self.session.with_token(token, |_| {
            let state = self
                .recommendation_state(source)
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let snapshot = state
                .latest
                .as_ref()
                .filter(|snapshot| snapshot.token == *token && snapshot.revision == revision)
                .ok_or(Error::QueueChanged)?;
            if index >= snapshot.ids.len() {
                return Err(Error::InvalidInput(
                    "invalid recommendation selection index",
                ));
            }
            Ok(snapshot.ids.clone())
        })?;
        let state = self.recommendation_state(source).clone();
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

fn recommendation_tracks(response: &Value, source: RecommendationSource) -> Result<TrackPage> {
    const MAX_TRACKS: usize = 500;
    let data = response["data"]
        .as_object()
        .ok_or(Error::Response("invalid recommendations"))?;
    let rows = match (source, data.get("dailySongs")) {
        (RecommendationSource::Style, None | Some(Value::Null)) => &[][..],
        (_, Some(Value::Array(rows))) if rows.len() <= MAX_TRACKS => rows.as_slice(),
        _ => return Err(Error::Response("invalid recommendation tracks")),
    };
    let mut ids = std::collections::HashSet::new();
    let tracks = rows
        .iter()
        .map(|row| {
            let track = mapping::track(row)?;
            if !ids.insert(track.id.clone()) {
                return Err(Error::Response("duplicate recommendation track"));
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
}
