use super::{Message, PlaybackStatus, Response, load::LoadOptions, state::Actor};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        quality_preferences::{
            QualityPreferences, QualityPreferencesSnapshot, QualityTrialBinding,
        },
        source::ResolvedSource,
    },
};
use serde_json::Value;
use std::sync::Arc;

const STAMP: &str = "quality_preference_revision";

pub(super) fn online_resource(resource: &Value) -> bool {
    resource.get("id").is_some()
        && resource.get("local_id").is_none()
        && resource.get("path").is_none()
}

pub(super) fn song_resource(resource: &Value) -> bool {
    online_resource(resource) && resource.get("voice_id").is_none_or(Value::is_null)
}

pub(super) struct QualityReady {
    pub id: String,
    pub generation: u64,
    pub token: SessionToken,
    pub entry_id: String,
    pub resource: Value,
    pub preference_revision: String,
    pub save_preference: bool,
    pub trial: Option<QualityTrialBinding>,
    pub result: Result<Arc<ResolvedSource>>,
    pub reply: Response,
}

impl Actor {
    pub(super) fn quality_sync(&mut self) -> Result<()> {
        self.quality_preferences = self.session.with_local_owner(&self.token, |owner| {
            QualityPreferencesSnapshot::read(&self.store, owner)
        })?;
        Ok(())
    }
    pub(super) fn quality_repair(&mut self) -> Result<()> {
        self.session.with_local_owner(&self.token, |owner| {
            QualityPreferences::repair(&self.store, owner)
        })?;
        self.quality_sync()
    }
    pub(super) fn stamp_quality(&self, resources: &mut [Value]) {
        for resource in resources.iter_mut().filter(|r| song_resource(r)) {
            if !self.quality_preferences.damaged
                && self.quality_preferences.preferences.revision != "initial"
                && resource.get("level").is_none_or(Value::is_null)
            {
                resource["level"] = self.quality_preferences.preferences.quality.as_str().into();
            }
            resource[STAMP] = self.quality_preferences.preferences.revision.clone().into();
        }
    }
    pub(super) fn apply_quality_preference(&self, resource: &mut Value) {
        let preferences = &self.quality_preferences;
        if song_resource(resource)
            && !preferences.damaged
            && preferences.preferences.revision != "initial"
            && resource[STAMP].as_str() != Some(&preferences.preferences.revision)
        {
            resource["level"] = preferences.preferences.quality.as_str().into();
            resource[STAMP] = preferences.preferences.revision.clone().into();
        }
    }
    pub fn trial_quality(
        &mut self,
        id: String,
        right: crate::playback::quality::QualityRight,
        operation: String,
        reply: Response,
    ) {
        let verified = crate::playback::quality::trials::QualityTrials::new(self.session.clone())
            .and_then(|trials| trials.record_for(&self.token, right))
            .is_ok_and(|record| record.is_some_and(|r| r.operation == operation && r.can_use()));
        if !verified
            || !self
                .queue
                .selected()
                .is_some_and(|s| song_resource(&s.resource))
        {
            self.reply(reply, Err(Error::State("quality trial is not active")));
            return;
        }
        let previous = self
            .quality_preferences
            .preferences
            .trial
            .as_ref()
            .map_or(self.quality_preferences.preferences.quality, |t| t.previous);
        self.quality(
            id,
            right.quality(),
            Some(QualityTrialBinding {
                operation,
                right,
                previous,
            }),
            reply,
        );
    }
    pub fn quality(
        &mut self,
        id: String,
        level: crate::membership::Quality,
        trial: Option<QualityTrialBinding>,
        reply: Response,
    ) {
        let Some(selected) = self.queue.selected() else {
            self.reply(reply, Err(Error::State("queue is empty")));
            return;
        };
        if !self.state.active()
            || !online_resource(&selected.resource)
            || (song_resource(&selected.resource) && self.quality_preferences.damaged)
        {
            self.reply(
                reply,
                Err(Error::State("quality requires loaded online track")),
            );
            return;
        }
        if let Some(task) = self.quality_task.take() {
            task.abort();
        }
        let save_preference = song_resource(&selected.resource);
        let mut resource = selected.resource;
        resource["level"] = level.as_str().into();
        let preference_revision = self.quality_preferences.preferences.revision.clone();
        let token = self.token.clone();
        let generation = self.generation();
        let resolver = self.resolver.clone();
        let sender = self.sender.clone();
        self.quality_id = Some(id.clone());
        self.quality_task = Some(tokio::spawn(async move {
            let result = resolver.resolve(&resource, &token).await;
            let _ = sender.send(Message::Quality(QualityReady {
                id,
                generation,
                token,
                entry_id: selected.entry_id,
                resource,
                preference_revision,
                save_preference,
                trial,
                result,
                reply,
            }));
        }));
    }
    pub async fn quality_ready(&mut self, mut ready: QualityReady) {
        if ready.reply.is_closed()
            || self.quality_id.as_deref() != Some(&ready.id)
            || self.generation() != ready.generation
            || self.session.token().ok().as_ref() != Some(&ready.token)
        {
            self.reply(ready.reply, Err(Error::StaleResponse));
            return;
        }
        self.quality_task = None;
        self.quality_id = None;
        let source = match ready.result {
            Ok(source) => source,
            Err(error) => {
                self.reply(ready.reply, Err(error));
                return;
            }
        };
        if self.queue.selected().as_ref().map(|i| i.entry_id.as_str()) != Some(&ready.entry_id) {
            self.reply(ready.reply, Err(Error::StaleResponse));
            return;
        }
        if ready.save_preference {
            let level = match crate::membership::Quality::parse(
                ready.resource["level"].as_str().unwrap_or(""),
            ) {
                Ok(level) => level,
                Err(error) => {
                    self.reply(ready.reply, Err(error));
                    return;
                }
            };
            let preference = match ready.trial {
                Some(trial) => QualityPreferences::fresh_trial(level, trial),
                None => QualityPreferences::fresh(level),
            };
            ready.resource[STAMP] = preference.revision.clone().into();
            let committed = self.session.with_local_owner(&self.token, |_| {
                self.queue.update_resource_with_quality(
                    &ready.entry_id,
                    ready.resource,
                    &ready.preference_revision,
                    &preference,
                )
            });
            if let Err(error) = committed {
                self.reply(ready.reply, Err(error));
                return;
            }
            self.quality_preferences = QualityPreferencesSnapshot {
                preferences: preference,
                damaged: false,
            };
        } else if let Err(error) = self.session.with_local_owner(&self.token, |_| {
            self.queue.update_resource(&ready.entry_id, ready.resource)
        }) {
            self.reply(ready.reply, Err(error));
            return;
        }
        let options = LoadOptions {
            paused: self.state == PlaybackStatus::Paused,
            position: self.position,
            recover: true,
            source: Some(source),
            ..Default::default()
        };
        // The resource was authorized before interrupting the old track.
        if let Err(error) = self.begin(options, ready.id, Some(ready.reply)).await {
            self.error = Some(error.to_string());
        }
    }
}
