//! M/308 and M/261: external resources are filtered before a local queue commit.
use super::*;
use crate::{
    library::{LocalLibrary, LocalRevision},
    playback::{
        PlaybackMode, QueueEntry, QueueSnapshot,
        controller::{PlaybackFence, TogetherExternalQueueUpdate},
    },
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use serde_json::Value;
use std::time::{Duration, Instant};

const NOTICE_LIFETIME: Duration = Duration::from_secs(60);

/// This private queue flag is projected only from native catalog metadata.
pub(in crate::client) const BLOCKED: &str = "_together_unsupported";
const EXPLICIT: &str = "_together_explicit_anchor";

pub(in crate::client) fn anchor(mut source: Value, explicit: bool) -> Value {
    if explicit {
        source[EXPLICIT] = Value::Bool(true);
    }
    source
}

/// M/21 projects the associated privilege's cs/sp and the song's t.
/// M/261 passes the *whole* privilege to M/35's Number & 16 check. Plain
/// JSON objects become NaN (bitwise zero); fee is a separate field.
pub(in crate::client) fn catalog_unsupported(song: &Value, supplied: Option<&Value>) -> bool {
    if ["programId", "voice_id"]
        .into_iter()
        .any(|field| js_truthy(&song[field]))
    {
        return true;
    }
    let privilege = supplied
        .filter(|v| v.is_object())
        .unwrap_or(&song["privilege"]);
    let is_cloud = privilege
        .get("cs")
        .map_or_else(|| number_is_one(&privilege["cloudSong"]), js_truthy);
    let private = song
        .get("t")
        .or_else(|| song.get("songType"))
        .is_some_and(number_is_one);
    let cannot_share = privilege
        .get("sp")
        .or_else(|| privilege.get("sharePriv"))
        .is_some_and(|value| !positive_number(value));
    (is_cloud && private) || cannot_share
}

fn number_is_one(value: &Value) -> bool {
    value.as_f64() == Some(1.0)
}

fn positive_number(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.as_f64().is_some_and(|n| n > 0.0),
        Value::Bool(b) => *b,
        Value::String(s) => {
            let s = s.trim();
            match s {
                "Infinity" | "+Infinity" => true,
                s if s.starts_with("0x") || s.starts_with("0X") => {
                    u64::from_str_radix(&s[2..], 16).is_ok_and(|n| n > 0)
                }
                s if s.starts_with("0b") || s.starts_with("0B") => {
                    u64::from_str_radix(&s[2..], 2).is_ok_and(|n| n > 0)
                }
                s if s.starts_with("0o") || s.starts_with("0O") => {
                    u64::from_str_radix(&s[2..], 8).is_ok_and(|n| n > 0)
                }
                s => s.parse::<f64>().is_ok_and(|n| n.is_finite() && n > 0.0),
            }
        }
        // M/21's normalized privilege comes from scalar server fields. Never
        // grant share eligibility based on an unexpected object/array value.
        _ => false,
    }
}

fn has_album(song: &Value) -> bool {
    song.get("al")
        .or_else(|| song.get("album"))
        .is_some_and(|album| identifier(&album["id"]).is_ok())
}

#[derive(Clone)]
pub(in crate::client) struct Scope {
    token: SessionToken,
    room: String,
    task: tokio::task::Id,
}

impl Scope {
    fn matches(&self, runtime: &Runtime) -> bool {
        runtime.token == self.token
            && runtime.room_id == self.room
            && runtime.worker.id() == self.task
            && !runtime.worker.is_finished()
    }
}

#[derive(Clone)]
pub(super) struct Notice {
    scope: Scope,
    id: String,
    created: Instant,
    skipped: usize,
    blocked_at: Option<PlaybackFence>,
}

impl Notice {
    fn active(&self) -> bool {
        self.created.elapsed() < NOTICE_LIFETIME
    }
}

pub(in crate::client) struct Filtered {
    scope: Scope,
    pub fence: PlaybackFence,
    skipped: usize,
    _gate: tokio::sync::OwnedMutexGuard<()>,
}

impl MusicClient {
    pub(in crate::client) fn together_external_room(
        &self,
        token: &SessionToken,
    ) -> Result<Option<Scope>> {
        let runtime = self
            .together
            .runtime
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        Ok(runtime
            .as_ref()
            .filter(|r| r.token == *token && !r.worker.is_finished())
            .map(|r| Scope {
                token: token.clone(),
                room: r.room_id.clone(),
                task: r.worker.id(),
            }))
    }

    pub(in crate::client) fn together_external_authority(
        &self,
        token: &SessionToken,
        scope: Option<Scope>,
    ) -> Cancellation {
        let token = token.clone();
        let runtime = Arc::downgrade(&self.together.runtime);
        Cancellation::new(move || {
            runtime.upgrade().is_some_and(|slot| {
                slot.lock().is_ok_and(|runtime| match &scope {
                    Some(scope) => runtime.as_ref().is_some_and(|r| scope.matches(r)),
                    None => runtime
                        .as_ref()
                        .is_none_or(|r| r.token != token || r.worker.is_finished()),
                })
            })
        })
    }

    pub(in crate::client) async fn filter_together_external(
        &self,
        token: &SessionToken,
        command: PlaybackCommand,
        expected: Option<&PlaybackFence>,
        source_authority: Option<&Cancellation>,
    ) -> Result<(PlaybackCommand, Option<Filtered>)> {
        let PlaybackCommand::Replace {
            resources,
            index,
            mut source,
        } = command
        else {
            return Ok((command, None));
        };
        let explicit = source
            .as_object_mut()
            .and_then(|source| source.remove(EXPLICIT))
            == Some(Value::Bool(true));
        if resources.is_empty() {
            return Ok((
                PlaybackCommand::Replace {
                    resources,
                    index,
                    source,
                },
                None,
            ));
        }
        if index >= resources.len() {
            return Err(Error::InvalidInput("invalid initial queue index"));
        }
        let Some(scope) = self.together_external_room(token)? else {
            return Ok((
                PlaybackCommand::Replace {
                    resources,
                    index,
                    source,
                },
                None,
            ));
        };
        let gate = tokio::time::timeout(
            Duration::from_secs(4),
            self.together.gate.clone().lock_owned(),
        )
        .await
        .map_err(|_| Error::MutationBusy)?;
        let current = self.together_external_authority(token, Some(scope.clone()));
        if current.is_cancelled() || source_authority.is_some_and(Cancellation::is_cancelled) {
            return Err(Error::StaleResponse);
        }
        let service = TogetherService::new(self.session.clone())?;
        let state = service.state_for(token)?;
        if state
            .room
            .as_ref()
            .is_none_or(|room| room.room_id != scope.room)
            || state.end_notice.is_some()
        {
            return Err(Error::StaleResponse);
        }
        let controller = self.player.controller().await?;
        let (snapshot, queue) = controller.queue_with_state(token).await?;
        let before = snapshot.fence();
        if expected.is_some_and(|expected| *expected != before) {
            return Err(Error::QueueChanged);
        }
        let (resources, index, skipped) =
            self.external_playlist_rows(token, resources, index, &source)?;
        self.session.check_generation(token)?;
        if current.is_cancelled() || source_authority.is_some_and(Cancellation::is_cancelled) {
            return Err(Error::StaleResponse);
        }
        if skipped > 0 && resources.is_empty() {
            self.set_external_notice(scope, skipped, Some(before))?;
            return Err(Error::TogetherSelectionUnsupported);
        }
        if resources.is_empty() {
            return Err(Error::State("selected song is unavailable"));
        }
        let command = if explicit && index.is_none() {
            PlaybackCommand::ApplyTogetherExternalQueue(TogetherExternalQueueUpdate {
                expected: before.clone(),
                queue: replace_list_without_start(queue, resources, source),
                room_id: scope.room.clone(),
            })
        } else {
            PlaybackCommand::Replace {
                resources,
                index: index.unwrap_or(0),
                source,
            }
        };
        Ok((
            command,
            Some(Filtered {
                scope,
                fence: before,
                skipped,
                _gate: gate,
            }),
        ))
    }

    fn external_playlist_rows(
        &self,
        token: &SessionToken,
        resources: Vec<Value>,
        index: usize,
        source: &Value,
    ) -> Result<(Vec<Value>, Option<usize>, usize)> {
        let local_ids = resources
            .iter()
            .filter_map(|r| r["local_id"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        let local = if local_ids.is_empty() {
            Default::default()
        } else {
            LocalLibrary::new(self.registry.store())?.online_mappings_for_collection(
                &self.session,
                token,
                &local_ids,
            )?
        };
        let mut kept = Vec::with_capacity(resources.len());
        let mut selected = None;
        let mut skipped = 0;
        for (ordinal, r) in resources.into_iter().enumerate() {
            if r["missing"] == true {
                // The official ordinary playback filter runs first.
                continue;
            }
            let song = &r["song"];
            let blocked = matches!(source["kind"].as_str(), Some("fm" | "zone_fm"))
                || matches!(source["scene"].as_str(), Some("fm" | "zoneFm"))
                || ["voice_id", "download_voice_id"]
                    .into_iter()
                    .any(|key| js_truthy(&r[key]))
                || catalog_unsupported(song, None)
                || song[BLOCKED] == true;
            let allowed = if blocked {
                false
            } else if let Some(id) = r["local_id"].as_str() {
                local.get(id).is_some_and(|link| {
                    let revision =
                        serde_json::from_value::<LocalRevision>(r["local_revision"].clone()).ok();
                    revision.as_ref() == Some(&link.revision)
                        && link.song.as_ref().is_some_and(|song| {
                            identifier(&song["id"]).ok().as_deref() == Some(&link.song_id)
                                && has_album(song)
                                && !catalog_unsupported(song, None)
                        })
                })
            } else if js_truthy(&r["download_task_id"]) {
                identifier(&r["id"]).is_ok()
                    && r["_together_catalog_download"] == true
                    && has_album(song)
            } else if js_truthy(&r["cloud_song_id"]) {
                // A private locker identity is not evidence of the wrapped
                // official onlineTrack/origin catalog identity.
                false
            } else {
                identifier(&r["id"]).is_ok()
            };
            if allowed {
                if ordinal == index {
                    selected = Some(kept.len());
                }
                kept.push(r);
            } else {
                skipped += 1;
            }
        }
        Ok((kept, selected, skipped))
    }

    fn set_external_notice(
        &self,
        scope: Scope,
        skipped: usize,
        blocked_at: Option<PlaybackFence>,
    ) -> Result<()> {
        let mut notice = self
            .together
            .external
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        *notice = (skipped > 0).then(|| Notice {
            scope,
            id: uuid::Uuid::new_v4().simple().to_string(),
            created: Instant::now(),
            skipped,
            blocked_at,
        });
        Ok(())
    }

    pub(in crate::client) fn finish_external_filter(&self, applied: Filtered) -> Result<()> {
        self.set_external_notice(applied.scope.clone(), applied.skipped, None)
    }

    pub(in crate::client) fn filtered_external_authority(
        &self,
        applied: &Filtered,
    ) -> Cancellation {
        self.together_external_authority(&applied.scope.token, Some(applied.scope.clone()))
    }

    pub(super) fn external_notice(
        &self,
        token: &SessionToken,
        room: Option<&str>,
    ) -> Result<Option<ExternalSelectionNotice>> {
        let slot = self
            .together
            .external
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        Ok(slot
            .as_ref()
            .filter(|v| {
                v.active()
                    && v.scope.token == *token
                    && Some(v.scope.room.as_str()) == room
                    && !self
                        .together_external_authority(token, Some(v.scope.clone()))
                        .is_cancelled()
            })
            .map(|v| ExternalSelectionNotice {
                id: v.id.clone(),
                room_id: v.scope.room.clone(),
                skipped: v.skipped,
                blocked: v.blocked_at.is_some(),
            }))
    }

    pub(super) async fn consume_external_notice(
        &self,
        token: &SessionToken,
        room: &str,
        id: &str,
        confirm: bool,
    ) -> Result<()> {
        let notice = self
            .together
            .external
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|v| {
                v.active()
                    && v.scope.token == *token
                    && v.scope.room == room
                    && v.id == id
                    && v.blocked_at.is_some()
            })
            .cloned()
            .ok_or(Error::StaleResponse)?;
        let authority = self.together_external_authority(token, Some(notice.scope));
        if authority.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        self.session.check_generation(token)?;
        if authority.is_cancelled() {
            return Err(Error::StaleResponse);
        }
        if confirm {
            let service = TogetherService::new(self.session.clone())?;
            let state = service.state_for(token)?;
            if state.end_notice.is_some()
                || state.room.as_ref().is_none_or(|r| r.room_id != room)
                || service
                    .operations_for(token)?
                    .iter()
                    .any(|op| op.phase.pending())
            {
                return Err(Error::StaleResponse);
            }
            let actor = self.player.controller().await?;
            let current = actor.command(token, PlaybackCommand::Snapshot).await?;
            if Some(current.fence()) != notice.blocked_at {
                return Err(Error::QueueChanged);
            }
        }
        *self
            .together
            .external
            .lock()
            .map_err(|_| Error::LockPoisoned)? = None;
        Ok(())
    }
}

fn same_entry(one: &Value, two: &Value) -> bool {
    if one["download_task_id"].is_string() || two["download_task_id"].is_string() {
        return one["download_task_id"].is_string()
            && one["download_task_id"] == two["download_task_id"];
    }
    if one["local_id"].is_string() || two["local_id"].is_string() {
        return one["local_id"].is_string()
            && one["local_id"] == two["local_id"]
            && one["local_revision"] == two["local_revision"];
    }
    one["id"].is_string()
        && one["id"] == two["id"]
        && ["voice_id", "cloud_song_id", "download_voice_id"]
            .iter()
            .all(|key| one[*key].is_null() && two[*key].is_null())
}

fn replace_list_without_start(
    before: QueueSnapshot,
    resources: Vec<Value>,
    source: Value,
) -> QueueSnapshot {
    use rand::seq::SliceRandom;
    let mut retained = before.selected();
    let current = retained.as_ref().map(|row| row.entry_id.clone());
    let mut entries = resources
        .into_iter()
        .map(|resource| {
            if retained
                .as_ref()
                .is_some_and(|row| same_entry(&row.resource, &resource))
            {
                let mut row = retained.take().expect("matched active row");
                if let Some(source) = row.source.as_object_mut() {
                    source.remove(crate::playback::intelligence::TRANSIENT);
                }
                return row;
            }
            QueueEntry {
                entry_id: uuid::Uuid::new_v4().simple().to_string(),
                resource,
                source: source.clone(),
            }
        })
        .collect::<Vec<_>>();
    let mut order = entries
        .iter()
        .map(|row| row.entry_id.clone())
        .collect::<Vec<_>>();
    if before.mode == PlaybackMode::Random {
        order.shuffle(&mut rand::rng());
    }
    if let Some(mut active) = retained {
        if !active.source.is_object() {
            active.source = json!({});
        }
        active.source[crate::playback::intelligence::TRANSIENT] = Value::Bool(true);
        order.insert(0, active.entry_id.clone());
        entries.insert(0, active);
    }
    QueueSnapshot {
        items: entries,
        order,
        current,
        mode: before.mode,
        ..QueueSnapshot::default()
    }
}
