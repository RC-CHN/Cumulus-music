use super::{RelayCommand, RelayConfig, RelayMode, RelayResource, RelayResourceKind, RelaySource};
use crate::{Error, Result};
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayMachine {
    source: RelaySource,
    session_id: String,
    current: RelayResource,
    progress_ms: u64,
    commands: Vec<RelayCommand>,
    snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RelayUpload {
    Source { retransmit: bool, value: Value },
    State(Value),
    Commands(Value),
}
impl RelayUpload {
    /// Only the explicit official stale-session response permits one fresh
    /// snapshot. A timeout, 403, or retransmission rejection never does.
    pub fn requires_snapshot(&self, error: &Error) -> bool {
        !matches!(
            self,
            Self::Source {
                retransmit: true,
                ..
            }
        ) && match error.root() {
            Error::Service { code, .. } => crate::error::code_matches(code, 10001),
            _ => false,
        }
    }
}
impl RelayMachine {
    pub fn start(source: RelaySource, current: RelayResource) -> Result<Self> {
        Self::with_session(
            source,
            current,
            Alphanumeric.sample_string(&mut rand::rng(), 12),
        )
    }
    /// Deterministic injection also supports recovered official test vectors.
    pub fn with_session(
        source: RelaySource,
        current: RelayResource,
        session_id: String,
    ) -> Result<Self> {
        let state = Self {
            source,
            current,
            session_id,
            progress_ms: 0,
            commands: vec![],
            snapshot: false,
        };
        state.validate()?;
        Ok(state)
    }
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.current.validate()?;
        if self.session_id.is_empty()
            || self.session_id.len() > 64
            || !self.session_id.bytes().all(|c| c.is_ascii_alphanumeric())
            || self.commands.len() > 512
        {
            return Err(Error::State("invalid persisted relay session"));
        }
        self.commands.iter().try_for_each(RelayCommand::validate)
    }
    pub fn current(&self) -> &RelayResource {
        &self.current
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn progress_ms(&self) -> u64 {
        self.progress_ms
    }
    pub fn source_upload(&self, mode: Option<RelayMode>, retransmit: bool) -> Result<RelayUpload> {
        self.validate()?;
        let mut value = json!({"retransmit": retransmit, "sessionId": self.session_id, "initResId": self.current.id, "playMode": mode.unwrap_or(RelayMode::ListLoop)});
        match &self.source {
            RelaySource::Collection { source_type, id } => {
                value["sourceType"] = json!(source_type);
                value["sourceId"] = json!(id);
            }
            RelaySource::Resources { resources } => value["resources"] = json!(resources),
        }
        Ok(RelayUpload::Source { retransmit, value })
    }
    pub fn state_upload(&self, mode: Option<RelayMode>) -> Result<RelayUpload> {
        self.validate()?;
        Ok(RelayUpload::State(
            json!({"resource":self.current,"progress":self.progress_ms,"sessionId":self.session_id,"playMode":mode.unwrap_or(RelayMode::Order)}),
        ))
    }
    pub fn commands_upload(&self) -> Result<RelayUpload> {
        self.validate()?;
        Ok(RelayUpload::Commands(
            json!({"commands":self.commands,"sessionId":self.session_id}),
        ))
    }
    /// Successful uploads intentionally retain cumulative commands (M/401).
    /// A new source/snapshot resets them. Overflow requires the caller's fresh
    /// authoritative queue snapshot; it must never silently drop old commands.
    pub fn command(&mut self, command: RelayCommand) -> Result<RelayUpload> {
        command.validate()?;
        self.validate()?;
        if self.commands.len() == 512 {
            return Err(Error::State("relay snapshot required"));
        }
        self.commands.push(command);
        self.commands_upload()
    }
    pub fn insert(&mut self, resources: Vec<RelayResource>) -> Result<RelayUpload> {
        self.command(RelayCommand::Insert {
            resources,
            pre_id: Some(self.current.id.clone()),
        })
    }
    /// Corrects official M/401's first-item-only, hardcoded-song bulk deletion.
    pub fn delete(&mut self, resources: Vec<RelayResource>) -> Result<RelayUpload> {
        self.command(RelayCommand::Delete { resources })
    }
    /// The owner removes this machine after persisting/sending the clear intent;
    /// failure never resurrects a queue belonging to an excluded scene.
    pub fn clear(&mut self) -> Result<RelayUpload> {
        self.command(RelayCommand::Clear)
    }
    pub fn select(
        &mut self,
        current: RelayResource,
        mode: Option<RelayMode>,
    ) -> Result<(RelayUpload, bool)> {
        current.validate()?;
        self.validate()?;
        let outside = self.snapshot
            && matches!(&self.source, RelaySource::Resources { resources } if !resources.contains(&current));
        // Official switches retain progress until a voice progress event arrives.
        self.current = current;
        Ok((self.state_upload(mode)?, outside))
    }
    pub fn voice_progress(
        &mut self,
        progress_ms: u64,
        mode: Option<RelayMode>,
    ) -> Result<RelayUpload> {
        self.validate()?;
        if self.current.kind != RelayResourceKind::Voice {
            return Err(Error::InvalidInput("relay progress is voice-only"));
        }
        self.progress_ms = progress_ms;
        self.state_upload(mode)
    }
    pub fn mode_change(&self, mode: RelayMode, from_handoff: bool) -> Result<Option<RelayUpload>> {
        if from_handoff {
            return Ok(None);
        }
        self.state_upload(Some(mode)).map(Some)
    }
    pub fn force_snapshot(&mut self, frame: SnapshotFrame<'_>) -> Result<RelayUpload> {
        self.force_snapshot_with_session(frame, Alphanumeric.sample_string(&mut rand::rng(), 12))
    }
    pub fn force_snapshot_with_session(
        &mut self,
        frame: SnapshotFrame<'_>,
        session_id: String,
    ) -> Result<RelayUpload> {
        let SnapshotFrame {
            items,
            selected_entry,
            current,
            config,
            mode,
        } = frame;
        self.validate()?;
        if items.len() > super::types::MAX_RESOURCES {
            return Err(Error::InvalidInput("relay queue too large"));
        }
        let index = items
            .iter()
            .position(|item| Some(item.entry_id.as_str()) == selected_entry)
            .unwrap_or(0);
        let range = snapshot_window(items.len(), index, config.snapshot_size);
        let resources = items[range]
            .iter()
            .map(|item| item.resource.clone())
            .collect();
        let mut next =
            Self::with_session(RelaySource::Resources { resources }, current, session_id)?;
        next.progress_ms = self.progress_ms;
        next.snapshot = true;
        let upload = next.source_upload(mode, true)?;
        *self = next;
        Ok(upload)
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotItem {
    pub entry_id: String,
    pub resource: RelayResource,
}
pub struct SnapshotFrame<'a> {
    pub items: &'a [SnapshotItem],
    pub selected_entry: Option<&'a str>,
    pub current: RelayResource,
    pub config: &'a RelayConfig,
    pub mode: Option<RelayMode>,
}

/// Exact recovered inclusive window, expressed as a Rust half-open range.
/// No edge backfill: size100 produces 50 rows at index0, 51 at the last index.
pub fn snapshot_window(len: usize, index: usize, size: usize) -> Range<usize> {
    if len == 0 || size == 0 {
        return 0..0;
    }
    let index = index.min(len - 1);
    index.saturating_sub(size / 2)..len.min(index.saturating_add(size.div_ceil(2)))
}

/// Native monotonic-clock counterpart of throttleTime + distinctUntilChanged.
/// The leading edge consumes a slot even for paused/song/mismatched samples,
/// matching the official pipeline's filtering AFTER the throttle.
#[derive(Default)]
pub struct VoiceProgressClock {
    last_tick_ms: Option<u64>,
    last_progress_ms: Option<u64>,
}
impl VoiceProgressClock {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn sample(
        &mut self,
        now_ms: u64,
        interval_secs: u64,
        progress_ms: u64,
        playing: bool,
        voice_matches: bool,
    ) -> Option<u64> {
        let interval = interval_secs.max(3).saturating_mul(1000);
        if self
            .last_tick_ms
            .is_some_and(|last| now_ms < last.saturating_add(interval))
        {
            return None;
        }
        self.last_tick_ms = Some(now_ms);
        if self.last_progress_ms == Some(progress_ms) {
            return None;
        }
        self.last_progress_ms = Some(progress_ms);
        (playing && voice_matches).then_some(progress_ms)
    }
}
