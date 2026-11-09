//! Optional song progress annotation, independent of source authorization.
mod fm;
mod schedule;
use super::{MusicClient, PlaybackCommand, PlaybackSnapshot, PlayerResourceKind, identity};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    playback::{
        QueueEntry, QueueSnapshot, TimelineMode, controller::PlaybackController,
        engine::Cancellation,
    },
    protocol::api::{Api, PlaybackChorusRead, identifier},
};
pub(super) use schedule::Host;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::OnceCell;

const DEADLINE: Duration = Duration::from_secs(5);

type Points = BTreeMap<String, f64>;
type Members = Vec<(String, i32, String)>;
type Batches = Vec<Vec<Value>>;

struct Plan {
    members: Members,
    batches: Batches,
}

fn eligible(entry: &QueueEntry) -> Option<(i32, String)> {
    if !entry.visible_in_queue()
        || identity::kind(&entry.resource) != PlayerResourceKind::Catalog
        || !entry.resource["url"].is_null()
        || !entry.resource["path"].is_null()
        || !entry.resource["download_ledger"].is_null()
        || !entry.resource["local_revision"].is_null()
        || !entry.resource["playback_constraint"].is_null()
        || entry.resource["missing"] == true
        || entry.source["kind"] == "listen_together"
    {
        return None;
    }
    let mode = match entry.resource.get("trial_mode") {
        None => -1,
        Some(mode) => i32::try_from(mode.as_i64()?).ok()?,
    };
    // M91's ordinary playback values. The FM bucket is a separate map in M602.
    if !matches!(mode, -1 | 35 | 37..=44 | 121) {
        return None;
    }
    let id = identifier(&entry.resource["id"]).ok()?;
    (id.len() <= 32).then_some((mode, id))
}

impl Plan {
    fn from_queue(queue: &QueueSnapshot, selected: &QueueEntry) -> Option<Self> {
        if eligible(selected).is_none()
            || queue.items.len() > crate::playback::intelligence::MAX_QUEUE_TRACKS
        {
            return None;
        }
        let mut members = Vec::new();
        let mut groups = BTreeMap::<u32, Vec<Value>>::new();
        let mut includes_current = false;
        for entry in &queue.items {
            let Some((mode, id)) = eligible(entry) else {
                continue;
            };
            includes_current |=
                entry.entry_id == selected.entry_id && entry.resource == selected.resource;
            members.push((entry.entry_id.clone(), mode, id));
            // M86 iterates JS object keys: nonnegative integer keys before "-1".
            groups
                .entry(mode as u32)
                .or_default()
                .push(entry.resource["id"].clone());
        }
        if !includes_current || members.len() < 2 {
            return None;
        }
        // Playback selection and reorder do not change the native membership.
        // An inserted or replaced entry gets a different key even for the same song.
        members.sort_unstable();
        let mut batches = Vec::new();
        for ids in groups.into_values() {
            batches.extend(ids.chunks(PlaybackChorusRead::LIMIT).map(<[Value]>::to_vec));
        }
        Some(Self { members, batches })
    }
}

struct Fill {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    plan: Plan,
    revision: AtomicU64,
    points: OnceCell<Points>,
}

impl Fill {
    fn matches(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: &str) -> bool {
        self.token == *token
            && self.api.ptr_eq(&Arc::downgrade(api))
            && self.credentials == credentials
    }
}

#[derive(Default)]
struct Cache(Mutex<Option<Arc<Fill>>>);

struct Scope {
    token: SessionToken,
    api: Arc<dyn Api>,
    credentials: String,
    room: Cancellation,
}

impl Scope {
    fn new(token: &SessionToken, api: Arc<dyn Api>, room: Cancellation) -> Option<Self> {
        if token.uid.is_none() || room.is_cancelled() {
            return None;
        }
        let credentials = api.playback_chorus_revision().ok()??;
        Some(Self {
            token: token.clone(),
            api,
            credentials,
            room,
        })
    }

    fn capture(token: &SessionToken, api: Arc<dyn Api>, room: Cancellation) -> Option<Self> {
        if token.uid.is_none() || room.is_cancelled() {
            return None;
        }
        let credentials = api.observed_playback_chorus_revision()?;
        Some(Self {
            token: token.clone(),
            api,
            credentials,
            room,
        })
    }

    fn check(&self, session: &AccountSession) -> Result<()> {
        session.require_api(&self.token, &self.api)?;
        self.room.check()?;
        if self.api.playback_chorus_revision()?.as_deref() != Some(self.credentials.as_str()) {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn plan(&self, current: &PlaybackSnapshot, queue: &QueueSnapshot) -> Option<Plan> {
        if current.account != self.token
            || current.fm
            || current.intelligence.is_some()
            || current.catalog_file_source
            || current.search_download_pending
        {
            return None;
        }
        Plan::from_queue(queue, current.selected.as_ref()?)
    }
}

impl Cache {
    fn observe(
        &self,
        session: &AccountSession,
        scope: &Scope,
        revision: u64,
        plan: Plan,
    ) -> Result<Arc<Fill>> {
        let mut slot = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        scope.check(session)?;
        if let Some(current) = slot
            .as_ref()
            .filter(|fill| fill.matches(&scope.token, &scope.api, &scope.credentials))
        {
            if revision < current.revision.load(Ordering::Acquire) {
                return Err(Error::StaleResponse);
            }
            if current.plan.members == plan.members {
                current.revision.store(revision, Ordering::Release);
                return Ok(current.clone());
            }
        }
        let fill = Arc::new(Fill {
            token: scope.token.clone(),
            api: Arc::downgrade(&scope.api),
            credentials: scope.credentials.clone(),
            plan,
            revision: AtomicU64::new(revision),
            points: OnceCell::new(),
        });
        *slot = Some(fill.clone());
        Ok(fill)
    }

    fn is_current(&self, fill: &Arc<Fill>) -> Result<bool> {
        Ok(self
            .0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, fill)))
    }
}

fn selected_song<'a>(snapshot: &'a PlaybackSnapshot, play_id: &str) -> Result<Option<&'a Value>> {
    if snapshot.play_id.as_deref() != Some(play_id) {
        return Err(Error::StaleResponse);
    }
    if !snapshot
        .timeline
        .as_ref()
        .is_some_and(|timeline| timeline.mode != TimelineMode::Cue && timeline.end.is_some())
    {
        return Ok(None);
    }
    Ok(snapshot.selected.as_ref().and_then(|entry| {
        if identity::kind(&entry.resource) != PlayerResourceKind::Catalog {
            return None;
        }
        let id = &entry.resource["id"];
        identifier(id).ok().map(|_| id)
    }))
}

fn start_seconds(response: &Value, song: &str) -> Option<f64> {
    response["data"].as_array()?.iter().find_map(|row| {
        if identifier(&row["id"]).ok().as_deref() != Some(song) {
            return None;
        }
        let start = row["startTime"].as_f64()? / 1000.0;
        (start.is_finite() && start > 0.0).then_some(start)
    })
}

impl Cache {
    async fn read(
        &self,
        session: &AccountSession,
        controller: &PlaybackController,
        scope: &Scope,
        current: &PlaybackSnapshot,
        plan: Plan,
        marker: Option<(&str, &Value)>,
    ) -> Result<Option<f64>> {
        let fill = self.observe(session, scope, current.queue_revision, plan)?;
        let mut changes = controller.subscribe();
        // Queue replies can precede the corresponding watch publication.
        tokio::time::timeout(
            DEADLINE,
            changes.wait_for(|snapshot| {
                snapshot.account == scope.token && snapshot.queue_revision >= current.queue_revision
            }),
        )
        .await
        .map_err(|_| Error::StaleResponse)?
        .map_err(|_| Error::Cancelled)?;
        let permission = || {
            scope.check(session)?;
            if !self.is_current(&fill)? {
                return Err(Error::StaleResponse);
            }
            let snapshot = changes.borrow();
            if snapshot.account != scope.token
                || snapshot.fm
                || snapshot.intelligence.is_some()
                || snapshot.catalog_file_source
                || snapshot.search_download_pending
                || snapshot.queue_revision != fill.revision.load(Ordering::Acquire)
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        let points = fill
            .points
            .get_or_try_init(|| async {
                let result = tokio::time::timeout(DEADLINE, async {
                    let updates = crate::playback::chorus::read(
                        &scope.api,
                        &fill.credentials,
                        &fill.plan.batches,
                        &permission,
                    )
                    .await?;
                    let mut points = Points::new();
                    crate::playback::chorus::merge(&mut points, updates);
                    Result::Ok(points)
                })
                .await;
                // No partial map. A batch failure stays empty for this queue;
                // a stale read may not install any result, even an empty one.
                permission()?;
                Ok::<_, Error>(result.ok().and_then(Result::ok).unwrap_or_default())
            })
            .await?;
        let Some((play_id, expected_song)) = marker else {
            return Ok(None);
        };
        let latest = controller
            .command(&scope.token, PlaybackCommand::Snapshot)
            .await?;
        permission()?;
        if selected_song(&latest, play_id)? != Some(expected_song)
            || latest.catalog_file_source
            || latest.queue_revision != fill.revision.load(Ordering::Acquire)
        {
            return Err(Error::StaleResponse);
        }
        Ok(points.get(&identifier(expected_song)?).copied())
    }
}

impl MusicClient {
    pub async fn player_chorus(&self, epoch: &str, play_id: &str) -> Result<Option<f64>> {
        let token = self.player_token(epoch)?;
        let controller = self.player.controller().await?;
        let (mut current, mut queue) = controller.queue_with_state(&token).await?;
        let Some(song) = selected_song(&current, play_id)?.cloned() else {
            return Ok(None);
        };
        let api = self.session.native_api(&token)?;
        if current.fm {
            return self
                .fm_chorus(controller, &current, &queue, play_id, &song, &api)
                .await;
        }
        if let Some(scope) = Scope::new(&token, api.clone(), self.together_detached_guard(&token))
            && let Some(mut plan) = scope.plan(&current, &queue)
        {
            if self
                .player
                .chorus
                .wait_for_startup(&self.session, &scope)
                .await?
            {
                (current, queue) = controller.queue_with_state(&token).await?;
                if selected_song(&current, play_id)? != Some(&song) {
                    return Err(Error::StaleResponse);
                }
                // The due read uses the actor's current queue, not the old
                // renderer observation held while waiting for startup.
                let Some(latest_plan) = scope.plan(&current, &queue) else {
                    return Ok(None);
                };
                plan = latest_plan;
            }
            return self
                .player
                .chorus
                .cache
                .read(
                    &self.session,
                    controller,
                    &scope,
                    &current,
                    plan,
                    Some((play_id, &song)),
                )
                .await;
        }
        let ids = serde_json::to_string(std::slice::from_ref(&song))?;
        // This optional decoration must not expire an otherwise working
        // playback session on a failed metadata read.
        let response = tokio::time::timeout(
            DEADLINE,
            api.call(
                PlaybackChorusRead::PATH,
                json!({ "ids": ids }),
                false,
                &[200],
            ),
        )
        .await
        .map_err(|_| Error::State("song chorus request timed out"))??;
        let latest = controller
            .command(&token, PlaybackCommand::Snapshot)
            .await?;
        self.session.require_api(&token, &api)?;
        if selected_song(&latest, play_id)? != Some(&song) {
            return Err(Error::StaleResponse);
        }
        Ok(start_seconds(&response, &identifier(&song)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_buckets_and_500_chunks_use_visible_catalog_entries_only() {
        let track = |n, trial| QueueEntry {
            entry_id: format!("e{n}"),
            resource: json!({"id":n,"trial_mode":trial}),
            source: Value::Null,
        };
        let mut queue = QueueSnapshot {
            items: (1..=501).map(|n| track(n, -1)).collect(),
            ..Default::default()
        };
        queue.items.push(track(700, 42));
        queue.items.push(track(702, 36));
        queue.items.push(track(703, 500));
        for (i, field) in ["local_id", "download_task_id", "voice_id", "cloud_song_id"]
            .into_iter()
            .enumerate()
        {
            let mut entry = track(704 + i, 42);
            entry.resource[field] = json!("21");
            queue.items.push(entry);
        }
        let current = queue.items[0].clone();
        let observed = Plan::from_queue(&queue, &current).unwrap();
        assert_eq!(observed.members.len(), 502);
        assert_eq!(
            observed.batches.iter().map(Vec::len).collect::<Vec<_>>(),
            [1, 500, 1]
        );
        assert_eq!(observed.batches[0], [json!(700)]);
        assert_eq!(observed.batches[1][0], json!(1));
        assert_eq!(observed.batches[2], [json!(501)]);
        queue.items.reverse();
        let reordered = Plan::from_queue(&queue, &current).unwrap();
        assert_eq!(observed.members, reordered.members);
        queue.items[0] = track(999, 42);
        assert_ne!(
            observed.members,
            Plan::from_queue(&queue, &current).unwrap().members
        );
    }

    #[test]
    fn chorus_requires_positive_milliseconds_for_the_requested_song() {
        let data = json!({ "data": [
            { "id": 11, "startTime": 4321 },
            { "id": 10, "startTime": 0 },
            { "id": 10, "startTime": "1234" },
            { "id": "10", "startTime": 1234 }
        ] });
        assert_eq!(start_seconds(&data, "10"), Some(1.234));
        assert_eq!(start_seconds(&data, "12"), None);
        assert_eq!(
            start_seconds(&json!({ "data": { "10": 1000 } }), "10"),
            None
        );
    }
}
