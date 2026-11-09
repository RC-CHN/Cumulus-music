use super::{
    MusicClient,
    dto::{AccountData, DailyRecommendationState, TrackPage},
    mapping,
    style_home::StyleSeedHandle,
};
use crate::{
    Error, Result, account::SessionToken, catalog::CatalogApi, playback::engine::Cancellation,
};
use chrono::{DateTime, Datelike, Local, TimeZone};
use serde_json::{Value, json};
use std::collections::VecDeque;

const CACHE_KIND: &str = "daily-recommendations";
const CACHE_KEY: &str = "tracks-v1";
const MAX_CACHE_BYTES: usize = 256 * 1024;
const MAX_TRACKS: usize = 500;
const MAX_STYLE_BACK_ENTRIES: usize = 8;
const MAX_STYLE_BACK_BYTES: usize = 1024 * 1024;

/// Use the same OS time zone for the historical birth date and today's date.
/// A February 29 birthday is only marked on an actual February 29.
fn birthday_today<Tz: TimeZone>(birthday_ms: Option<i64>, now: DateTime<Tz>) -> bool {
    // Keep arbitrary profile values away from Chrono's representational edges.
    const FIRST_COMMON_ERA_MILLIS: i64 = -62_135_596_800_000;
    let Some(birthday) = birthday_ms
        .filter(|ms| *ms != 0 && (FIRST_COMMON_ERA_MILLIS..=now.timestamp_millis()).contains(ms))
        .and_then(|ms| now.timezone().timestamp_millis_opt(ms).single())
    else {
        return false;
    };
    let (birthday, today) = (birthday.date_naive(), now.date_naive());
    birthday <= today && birthday.month() == today.month() && birthday.day() == today.day()
}

#[derive(Default)]
pub(super) struct RecommendationSnapshot {
    started: u64,
    latest: Option<Snapshot>,
    retained: VecDeque<Snapshot>,
}

impl RecommendationSnapshot {
    fn start(&mut self) -> Result<u64> {
        self.started = self
            .started
            .checked_add(1)
            .ok_or(Error::State("recommendation revision exhausted"))?;
        Ok(self.started)
    }

    fn replace(&mut self, snapshot: Snapshot) {
        self.retained.retain(|prior| prior.token == snapshot.token);
        if let Some(prior) = self.latest.take()
            && prior.token == snapshot.token
            && prior.style_back.is_some()
        {
            self.retained.push_back(prior);
        }
        if self.retained.len() > MAX_STYLE_BACK_ENTRIES {
            self.retained.pop_front();
        }
        self.latest = Some(snapshot);
    }
}

struct Snapshot {
    token: SessionToken,
    revision: String,
    ids: Vec<Value>,
    style_preference: Option<super::style_tags::StylePreference>,
    style_back: Option<StyleBackPage>,
    daily_preview: Vec<super::dto::TrackView>,
}

struct StyleBackPage {
    seed: Option<StyleSeedHandle>,
    page: TrackPage,
}

#[derive(Clone, Copy)]
pub(super) enum RecommendationSource {
    Daily,
    Style,
    SeededStyle,
}

impl RecommendationSource {
    fn revision(self, request: u64) -> String {
        let prefix = match self {
            Self::Daily => "daily",
            Self::Style => "style",
            Self::SeededStyle => "seeded-style",
        };
        format!("{prefix}-{request}-{}", uuid::Uuid::new_v4().simple())
    }

    pub(super) fn style_for_revision(revision: &str) -> Self {
        if revision.starts_with("seeded-style-") {
            Self::SeededStyle
        } else {
            Self::Style
        }
    }
}

pub(super) fn daily_source(revision: &str) -> Value {
    json!({"kind":"daily_recommendations","scene":"dailyRecommend",
        "href":"/dailyRecommend","resourceType":"dailyRecommend",
        "revision":revision,"trial_mode":35})
}

pub(super) fn style_source(revision: &str) -> Value {
    // Official M/3126 uses the dailyRecommend resource and trial mode (M/91: 35).
    json!({"kind":"style_recommendations","scene":"styleRecommend",
        "href":"/styleRecommend","resourceType":"dailyRecommend",
        "revision":revision,"trial_mode":35})
}

pub(super) fn daily_subset(ids: Vec<Value>, indices: Option<&[usize]>) -> Result<Vec<Value>> {
    let Some(indices) = indices else {
        return Ok(ids);
    };
    if indices.is_empty() || indices.len() > ids.len() {
        return Err(Error::InvalidInput(
            "invalid daily recommendation selection size",
        ));
    }
    let mut selected = std::collections::HashSet::with_capacity(indices.len());
    for &index in indices {
        if index >= ids.len() || !selected.insert(index) {
            return Err(Error::InvalidInput(
                "invalid daily recommendation selection index",
            ));
        }
    }
    Ok(ids
        .into_iter()
        .enumerate()
        .filter_map(|(index, id)| selected.contains(&index).then_some(id))
        .collect())
}

impl MusicClient {
    fn recommendation_state(
        &self,
        source: RecommendationSource,
    ) -> &std::sync::Arc<std::sync::Mutex<RecommendationSnapshot>> {
        match source {
            RecommendationSource::Daily => &self.daily_recommendations,
            RecommendationSource::Style => &self.style_recommendations,
            RecommendationSource::SeededStyle => &self.seeded_style_recommendations,
        }
    }

    fn begin_recommendation(&self, source: RecommendationSource) -> Result<u64> {
        let mut state = self
            .recommendation_state(source)
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        state.start()
    }

    pub(super) fn daily_notification_cached_preview(
        &self,
        token: &SessionToken,
    ) -> Result<Vec<super::dto::TrackView>> {
        self.session.with_token(token, |_| {
            let state = self
                .daily_recommendations
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            Ok(state
                .latest
                .as_ref()
                .filter(|page| page.token == *token)
                .map(|page| page.daily_preview.clone())
                .unwrap_or_default())
        })
    }

    pub(super) fn daily_notification_persisted_preview(
        &self,
        token: &SessionToken,
    ) -> Result<Vec<super::dto::TrackView>> {
        self.session.with_token(token, |owner| {
            Ok(self
                .registry
                .store()
                .get(owner, CACHE_KIND, CACHE_KEY)?
                .and_then(|value| track_page(value.as_array()?).ok())
                .map(|page| page.tracks.into_iter().take(3).collect())
                .unwrap_or_default())
        })
    }

    pub async fn daily_songs(&self) -> Result<AccountData<TrackPage>> {
        let source = RecommendationSource::Daily;
        let token = self.session.token()?;
        if token.uid.is_none() {
            return Err(Error::service("session", json!(301), Value::Null));
        }
        let request = self.begin_recommendation(source)?;
        let endpoint = self.daily_recommendation_endpoint(&token).await?;
        let fresh = self
            .session
            .run_for(&token, |api, _| async move {
                let reply = CatalogApi::new(api).daily_for(endpoint).await?;
                Ok((track_page(&reply.songs)?, reply.show_free_tag))
            })
            .await;
        let (mut value, cached, show_free_tag) = match fresh {
            Ok((value, show_free_tag)) => (value, false, show_free_tag),
            Err(error) if can_use_daily_cache(&error) => {
                let cached = self.session.with_token(&token, |owner| {
                    Ok(self
                        .registry
                        .store()
                        .get(owner, CACHE_KIND, CACHE_KEY)
                        .ok()
                        .flatten()
                        .and_then(|value| track_page(value.as_array()?).ok())
                        .filter(|page| !page.tracks.is_empty()))
                })?;
                (cached.ok_or(error)?, true, true)
            }
            Err(error) => return Err(error),
        };
        let profile = self.session.snapshot()?;
        if profile.token != token {
            return Err(Error::StaleResponse);
        }
        let birthday = profile
            .profile
            .and_then(|profile| profile["birthday"].as_i64());
        value.daily = Some(DailyRecommendationState {
            cached,
            show_free_tag,
            birthday_today: birthday_today(birthday, Local::now()),
        });
        self.publish_recommendation(source, request, token, value, None)
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
                style_tracks(&response)
            })
            .await?;
        self.publish_recommendation(source, request, token, value, None)
    }

    pub async fn seeded_style_recommendations(
        &self,
        epoch: &str,
        handle: StyleSeedHandle,
    ) -> Result<AccountData<TrackPage>> {
        let token = self.player_token(epoch)?;
        let source = RecommendationSource::SeededStyle;
        let (request, seed) = self.session.with_token(&token, |_| {
            let seed = self.style_seed(&token, &handle)?;
            Ok((self.begin_recommendation(source)?, seed))
        })?;
        let value = self
            .session
            .run_for(&token, |api, _| async move {
                let response = CatalogApi::new(api)
                    .seeded_style_recommendations(seed.request())
                    .await?;
                style_tracks(&response)
            })
            .await?;
        self.publish_recommendation(source, request, token, value, Some(&handle))
    }

    fn publish_recommendation(
        &self,
        source: RecommendationSource,
        request: u64,
        token: SessionToken,
        mut value: TrackPage,
        seed: Option<&StyleSeedHandle>,
    ) -> Result<AccountData<TrackPage>> {
        self.session.with_token(&token, |owner| {
            if let Some(handle) = seed {
                self.style_seed(&token, handle)?;
            }
            let mut state = self
                .recommendation_state(source)
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            if state.started != request {
                return Err(Error::StaleResponse);
            }
            if value.daily.as_ref().is_some_and(|daily| !daily.cached) && !value.tracks.is_empty() {
                let projected = Value::Array(
                    value
                        .tracks
                        .iter()
                        .map(mapping::projected_song_metadata)
                        .collect(),
                );
                if serde_json::to_vec(&projected).is_ok_and(|bytes| bytes.len() <= MAX_CACHE_BYTES)
                {
                    // Cache is opportunistic; an unwritable cache cannot invalidate a fresh read.
                    let _ = self
                        .registry
                        .store()
                        .put(owner, CACHE_KIND, CACHE_KEY, &projected);
                }
            }
            // Queue origins survive app restarts; an in-memory counter alone
            // cannot distinguish their snapshots from new recommendations.
            let revision = source.revision(request);
            value.revision = Some(revision.clone());
            let style_back = (!matches!(source, RecommendationSource::Daily)
                && serde_json::to_vec(&value)
                    .is_ok_and(|bytes| bytes.len() <= MAX_STYLE_BACK_BYTES))
            .then(|| StyleBackPage {
                seed: seed.cloned(),
                page: value.clone(),
            });
            state.replace(Snapshot {
                token: token.clone(),
                revision,
                ids: value.tracks.iter().map(|row| json!(row.id)).collect(),
                style_preference: if matches!(source, RecommendationSource::Style) {
                    value
                        .style_tags
                        .as_ref()
                        .map(super::style_tags::StylePreference::from_category)
                } else {
                    None
                },
                style_back,
                daily_preview: if matches!(source, RecommendationSource::Daily) {
                    value.tracks.iter().take(3).cloned().collect()
                } else {
                    vec![]
                },
            });
            Ok(AccountData {
                epoch: token.generation.to_string(),
                value,
            })
        })
    }

    /// Restore only a page this account actually received. A restored previous
    /// page receives a fresh active playback revision; stale rows stay stale.
    pub fn restore_style_recommendations(
        &self,
        epoch: &str,
        revision: &str,
        seed: Option<&StyleSeedHandle>,
    ) -> Result<Option<AccountData<TrackPage>>> {
        if revision.is_empty() || revision.len() > 96 {
            return Err(Error::InvalidInput("invalid style recommendation revision"));
        }
        let token = self.player_token(epoch)?;
        let source = if seed.is_some() {
            RecommendationSource::SeededStyle
        } else {
            RecommendationSource::Style
        };
        self.session.with_token(&token, |_| {
            let mut state = self
                .recommendation_state(source)
                .lock()
                .map_err(|_| Error::LockPoisoned)?;
            let matches = |snapshot: &Snapshot| {
                snapshot.token == token
                    && snapshot.revision == revision
                    && snapshot
                        .style_back
                        .as_ref()
                        .is_some_and(|value| value.seed.as_ref() == seed)
            };
            let value = if let Some(page) = state
                .latest
                .as_ref()
                .filter(|snapshot| matches(snapshot))
                .and_then(|snapshot| snapshot.style_back.as_ref())
                .map(|value| value.page.clone())
            {
                // Back is more recent than a previously started style read.
                state.start()?;
                page
            } else if let Some(index) = state.retained.iter().position(matches) {
                let request = state.start()?;
                let mut snapshot = state.retained.remove(index).ok_or(Error::StaleResponse)?;
                let back = snapshot.style_back.as_mut().ok_or(Error::StaleResponse)?;
                snapshot.revision = source.revision(request);
                back.page.revision = Some(snapshot.revision.clone());
                let page = back.page.clone();
                state.replace(snapshot);
                page
            } else {
                return Ok(None);
            };
            Ok(Some(AccountData {
                epoch: token.generation.to_string(),
                value,
            }))
        })
    }

    /// The caller holds the session fence. Preference mutations invalidate
    /// only unseeded back pages, while the visible queue can still play.
    pub(super) fn invalidate_style_back_pages(&self, token: &SessionToken) -> Result<()> {
        let mut state = self
            .recommendation_state(RecommendationSource::Style)
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        state.start()?;
        state.retained.clear();
        if let Some(active) = state
            .latest
            .as_mut()
            .filter(|active| active.token == *token)
        {
            active.style_back = None;
        }
        Ok(())
    }

    /// Caller already holds the session fence when using this for a write.
    pub(super) fn style_preference_at(
        &self,
        token: &SessionToken,
        revision: &str,
    ) -> Result<super::style_tags::StylePreference> {
        let state = self
            .style_recommendations
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let snapshot = state
            .latest
            .as_ref()
            .filter(|snapshot| snapshot.token == *token && snapshot.revision == revision)
            .ok_or(Error::QueueChanged)?;
        snapshot
            .style_preference
            .clone()
            .ok_or(Error::State("applied style preference unavailable"))
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

fn style_tracks(response: &Value) -> Result<TrackPage> {
    let data = response["data"]
        .as_object()
        .ok_or(Error::Response("invalid recommendations"))?;
    let rows = match data.get("dailySongs") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(rows)) => rows.as_slice(),
        _ => return Err(Error::Response("invalid recommendation tracks")),
    };
    let mut page = track_page(rows)?;
    page.style_tags = super::style_tags::applied(data.get("tags"));
    Ok(page)
}

pub(super) fn track_page(rows: &[Value]) -> Result<TrackPage> {
    if rows.len() > MAX_TRACKS {
        return Err(Error::Response("too many recommendation tracks"));
    }
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
        playlist_local: None,
        playlist_offline: None,
        daily: None,
        style_tags: None,
    })
}

pub(super) fn can_use_daily_cache(error: &Error) -> bool {
    match error.root() {
        Error::Transport(_) | Error::Response(_) => true,
        Error::HttpStatus { status, .. } => {
            matches!(status, 408 | 429 | 500..=599)
                && ![301, 401, 403]
                    .into_iter()
                    .any(|code| error.is_service_code(code))
        }
        Error::Service { .. } => [408, 429, 500, 502, 503, 504]
            .into_iter()
            .any(|code| error.is_service_code(code)),
        _ => false,
    }
}

#[cfg(test)]
mod birthday_tests {
    use super::birthday_today;
    use chrono::{FixedOffset, TimeZone, Utc};

    #[test]
    fn compares_the_two_local_calendar_dates_instead_of_utc_or_month_indices() {
        let tz = FixedOffset::east_opt(8 * 3600).unwrap();
        let birthday = Utc
            .with_ymd_and_hms(2000, 11, 3, 17, 0, 0)
            .unwrap()
            .timestamp_millis();
        let today = tz.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap();
        assert!(birthday_today(Some(birthday), today));
        assert!(!birthday_today(Some(birthday), today.with_timezone(&Utc)));
        let previous_day = tz.with_ymd_and_hms(2026, 11, 3, 23, 59, 0).unwrap();
        assert!(!birthday_today(Some(birthday), previous_day));
        let wrong_month = tz.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
        assert!(!birthday_today(Some(birthday), wrong_month));
    }

    #[test]
    fn missing_unset_invalid_and_future_dates_do_not_trigger_a_greeting() {
        let now = Utc.with_ymd_and_hms(2026, 2, 28, 12, 0, 0).unwrap();
        let future = Utc
            .with_ymd_and_hms(2027, 2, 28, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        for value in [None, Some(0), Some(i64::MIN), Some(i64::MAX), Some(future)] {
            assert!(!birthday_today(value, now));
        }
        let leap = Utc
            .with_ymd_and_hms(2000, 2, 29, 12, 0, 0)
            .unwrap()
            .timestamp_millis();
        assert!(!birthday_today(Some(leap), now));
        assert!(birthday_today(
            Some(leap),
            Utc.with_ymd_and_hms(2028, 2, 29, 12, 0, 0).unwrap()
        ));
    }
}
