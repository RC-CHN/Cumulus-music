//! M/3161 command reduction. Clock values are supplied by the host so tests and
//! adapters can use one monotonic epoch. Room changes must reset this reducer.
use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandKind {
    Play,
    Pause,
    Progress,
    Next,
    Previous,
    Goto,
}
impl CommandKind {
    fn starts(self) -> bool {
        matches!(self, Self::Play | Self::Next | Self::Previous | Self::Goto)
    }
    fn selects(self) -> bool {
        matches!(self, Self::Next | Self::Previous | Self::Goto)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlayState {
    Play,
    Pause,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayCommand {
    pub command_type: CommandKind,
    pub progress: u64,
    pub play_status: PlayState,
    pub former_song_id: String,
    pub target_song_id: String,
    pub client_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}
impl PlayCommand {
    /// The argument is full-song time, including any trial-window offset.
    pub fn at_seconds(
        kind: CommandKind,
        seconds: f64,
        state: PlayState,
        former: &str,
        target: &str,
    ) -> Result<Self> {
        if !seconds.is_finite() || seconds < 0.0 || seconds * 1000.0 > 9_007_199_254_740_991.0 {
            return Err(Error::InvalidInput("invalid full-song together position"));
        }
        let progress = (seconds * 1000.0).round() as u64;
        let command_type = if kind == CommandKind::Progress && former == target && progress == 0 {
            CommandKind::Goto
        } else {
            kind
        };
        let command = Self {
            command_type,
            progress,
            play_status: state,
            former_song_id: former.into(),
            target_song_id: target.into(),
            client_seq: 0,
            server_seq: None,
            user_id: None,
        };
        command.validate()?;
        Ok(command)
    }
    pub fn validate(&self) -> Result<()> {
        identifier(&json!(self.target_song_id))?;
        // The initial local player can have no former song.
        if !self.former_song_id.is_empty() {
            identifier(&json!(self.former_song_id))?;
        }
        if let Some(id) = &self.user_id {
            identifier(&json!(id))?;
        }
        if self.progress > 9_007_199_254_740_991 {
            return Err(Error::InvalidInput(
                "together position exceeds exact milliseconds",
            ));
        }
        Ok(())
    }
    pub fn from_official(v: &Value) -> Result<Self> {
        let mut v = v.clone();
        for field in ["formerSongId", "targetSongId", "userId"] {
            if !v[field].is_null() && v[field] != "" {
                v[field] = json!(super::model::id(&v[field])?);
            }
        }
        let command: Self = serde_json::from_value(v)?;
        command.validate()?;
        Ok(command)
    }
    pub fn wire(&self) -> Result<Value> {
        self.validate()?;
        Ok(
            json!({"commandType":self.command_type,"progress":self.progress,"playStatus":self.play_status,
            "formerSongId":self.former_song_id,"targetSongId":self.target_song_id,"clientSeq":self.client_seq}),
        )
    }
    pub fn same_report(&self, other: &Self) -> bool {
        self.command_type == other.command_type
            && self.progress == other.progress
            && self.play_status == other.play_status
            && self.former_song_id == other.former_song_id
            && self.target_song_id == other.target_song_id
            && self.client_seq == other.client_seq
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedCommand {
    pub command: PlayCommand,
    pub at_ms: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlaybackSync {
    pub client_seq: u64,
    pub latest: Option<TimedCommand>,
    pub received: Option<TimedCommand>,
    pub sent: Vec<TimedCommand>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}
fn recent(now: u64, then: u64, window: u64) -> bool {
    now.checked_sub(then).is_some_and(|age| age < window)
}
impl PlaybackSync {
    /// Port of cmdFilter, including its progress truthiness in the outgoing
    /// echo check. Restore below intentionally accepts progress=0.
    pub fn filtered(
        &self,
        direction: Direction,
        command: &PlayCommand,
        allowed: &[String],
        playing: bool,
        now: u64,
    ) -> bool {
        if !allowed.contains(&command.target_song_id) {
            return true;
        }
        let peers: Vec<_> = if direction == Direction::In {
            self.sent
                .iter()
                .filter(|v| recent(now, v.at_ms, 5000))
                .collect()
        } else {
            self.received.iter().collect()
        };
        let target = &command.target_song_id;
        let kind = command.command_type;
        if direction == Direction::In
            && peers.iter().any(|v| {
                v.command.client_seq == command.client_seq
                    && v.command.command_type == kind
                    && v.command.target_song_id == *target
                    && v.command.progress == command.progress
            })
        {
            return true;
        }
        if direction == Direction::Out
            && peers.iter().any(|v| {
                v.command.command_type == kind
                    && v.command.target_song_id == *target
                    && recent(now, v.at_ms, 1000)
                    && v.command.progress != 0
                    && v.command.progress.abs_diff(command.progress) < 1000
            })
        {
            return true;
        }
        if let Some(v) = &self.latest {
            let previous = v.command.command_type;
            if v.command.target_song_id == *target {
                if v.command.progress.abs_diff(command.progress) < 1000
                    && kind == previous
                    && (recent(now, v.at_ms, 1000) || previous == CommandKind::Pause)
                {
                    return true;
                }
                if !matches!(kind, CommandKind::Progress | CommandKind::Play)
                    && ((previous == CommandKind::Pause && kind == CommandKind::Pause)
                        || (!matches!(previous, CommandKind::Progress | CommandKind::Pause)
                            && kind != CommandKind::Pause))
                {
                    return true;
                }
            }
        }
        peers.iter().any(|v| {
            (kind == CommandKind::Progress
                || (kind.starts() && (playing || v.command.command_type.starts()))
                || (kind == CommandKind::Pause
                    && (playing || v.command.command_type == CommandKind::Pause)))
                && v.command.progress.abs_diff(command.progress) < 1000
                && recent(now, v.at_ms, 1000)
                && v.command.target_song_id == *target
        })
    }
    /// A suppressed command is not sent. Sequence increments before filtering,
    /// as in M/3161; the journal must persist it before any HTTP attempt.
    pub fn outgoing(
        &mut self,
        command: PlayCommand,
        allowed: &[String],
        playing: bool,
        now: u64,
    ) -> Result<Option<PlayCommand>> {
        self.outgoing_with_origin(command, allowed, playing, now, false)
    }
    /// Explicit local seek intent. The official wire conversion changes a
    /// same-song PROGRESS=0 to GOTO, whose start-command filter then incorrectly
    /// suppresses it after PLAY/GOTO. Filter its seek meaning, retain GOTO on
    /// the wire, and preserve echo, allowed-song and short duplicate guards.
    pub fn outgoing_seek(
        &mut self,
        command: PlayCommand,
        allowed: &[String],
        playing: bool,
        now: u64,
    ) -> Result<Option<PlayCommand>> {
        if command.command_type != CommandKind::Progress
            && !(command.command_type == CommandKind::Goto
                && command.progress == 0
                && command.former_song_id == command.target_song_id)
        {
            return Err(Error::InvalidInput(
                "explicit seek requires a position command",
            ));
        }
        self.outgoing_with_origin(command, allowed, playing, now, true)
    }
    fn outgoing_with_origin(
        &mut self,
        mut command: PlayCommand,
        allowed: &[String],
        playing: bool,
        now: u64,
        explicit_seek: bool,
    ) -> Result<Option<PlayCommand>> {
        command.validate()?;
        if self.latest.as_ref().is_some_and(|v| {
            v.command.target_song_id == command.target_song_id
                && ((command.command_type == CommandKind::Play
                    && v.command.play_status == PlayState::Play)
                    || (command.command_type == CommandKind::Pause
                        && v.command.play_status == PlayState::Pause))
        }) {
            return Ok(None);
        }
        self.client_seq = self
            .client_seq
            .checked_add(1)
            .ok_or(Error::State("together command sequence exhausted"))?;
        command.client_seq = self.client_seq;
        let mut filter_command = command.clone();
        let zero_seek = explicit_seek && command.command_type == CommandKind::Goto;
        if zero_seek {
            filter_command.command_type = CommandKind::Progress;
        }
        let repeated_zero = zero_seek
            && self.latest.as_ref().is_some_and(|old| {
                old.command.target_song_id == command.target_song_id
                    && old.command.progress == 0
                    && (recent(now, old.at_ms, 1000)
                        || (old.command.play_status == PlayState::Pause
                            && command.play_status == PlayState::Pause))
            });
        if repeated_zero || self.filtered(Direction::Out, &filter_command, allowed, playing, now) {
            return Ok(None);
        }
        self.sent.retain(|v| recent(now, v.at_ms, 5000));
        let timed = TimedCommand {
            command: command.clone(),
            at_ms: now,
        };
        self.latest = Some(timed.clone());
        self.sent.push(timed);
        Ok(Some(command))
    }
    pub fn incoming(
        &mut self,
        command: PlayCommand,
        allowed: &[String],
        current: Option<&str>,
        playing: bool,
        now: u64,
    ) -> Result<bool> {
        command.validate()?;
        let sequence = command
            .server_seq
            .ok_or(Error::Response("incoming command lacks server sequence"))?;
        if self
            .received
            .as_ref()
            .and_then(|v| v.command.server_seq)
            .is_some_and(|s| s >= sequence)
            || self.filtered(Direction::In, &command, allowed, playing, now)
            || (!command.command_type.selects() && current != Some(command.target_song_id.as_str()))
        {
            return Ok(false);
        }
        let timed = TimedCommand {
            command,
            at_ms: now,
        };
        self.latest = Some(timed.clone());
        self.received = Some(timed);
        Ok(true)
    }
    pub fn restore(&mut self, command: PlayCommand, owner: &str, now: u64) -> Result<()> {
        command.validate()?;
        let uid = command
            .user_id
            .as_deref()
            .ok_or(Error::Response("restored command lacks user identity"))?;
        let own = uid == owner;
        if own {
            self.client_seq = self.client_seq.max(command.client_seq);
        }
        // Read refreshes may lag behind an IM delivery. Never rewind its
        // server watermark or restart duplicate-filter clocks on a repeated
        // snapshot; otherwise a delayed message could be applied twice.
        if let Some(sequence) = command.server_seq
            && self
                .latest
                .iter()
                .chain(self.received.iter())
                .any(|old| old.command.server_seq.is_some_and(|seen| seen >= sequence))
        {
            return Ok(());
        }
        let timed = TimedCommand {
            command,
            at_ms: now,
        };
        self.latest = Some(timed.clone());
        if own {
            self.sent = vec![timed];
        } else {
            self.received = Some(timed);
        }
        Ok(())
    }
}
