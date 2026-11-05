use super::StereoPcmReader;
use crate::{
    Error, Result,
    download::offline::ColdOfflineFlacAuthority,
    effects::stereo::{LocalStereoMaterialPlan, LocalStereoSubgraph},
    playback::engine::Cancellation,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) const QUANTUM: usize = 2048;
const WAIT: Duration = Duration::from_millis(20);
const MAX_CONTROLS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StereoPcmSnapshot {
    pub first_source_frame: u64,
    pub source_end: u64,
    pub decoded_source_frame: u64,
    pub produced_frames: u64,
    pub delivered_frames: u64,
    pub buffered_frames: u32,
    pub native_output_frame: u64,
    /// Derived only from the bound native clock, never produced/delivered PCM.
    pub native_source_frame: u64,
    pub native_entry: Option<i64>,
    pub paused: bool,
    pub seeking: bool,
    pub graph_end: bool,
    pub requested_revision: u64,
    pub applied_revision: u64,
    pub cancelled: bool,
    pub failed: bool,
    pub reader_closed: bool,
    pub native_sink_retired: bool,
}

pub(super) struct State {
    pub(super) pcm: VecDeque<f32>,
    pub(super) decoded: u64,
    pub(super) produced: u64,
    pub(super) delivered: u64,
    pub(super) native: u64,
    pub(super) entry: Option<i64>,
    pub(super) paused: bool,
    seeking: bool,
    pub(super) end: bool,
    pub(super) failed: bool,
    controls: VecDeque<(u64, LocalStereoMaterialPlan)>,
    requested: u64,
    applied: u64,
}

pub(crate) struct Shared {
    pub(super) state: Mutex<State>,
    pub(super) wake: Condvar,
    pub(super) cancelled: AtomicBool,
    failed: AtomicBool,
    pub(super) reader_closed: AtomicBool,
    sink_retired: AtomicBool,
    pub(super) authority: Cancellation,
    host_authority: Option<ColdOfflineFlacAuthority>,
    rate: u32,
    first: u64,
    end: u64,
    capacity: usize,
    startup: u64,
    lead: u64,
}

#[derive(Clone)]
pub struct StereoPcmControl(pub(crate) Arc<Shared>);

pub(crate) fn channel(
    rate: u32,
    first: u64,
    end: u64,
    authority: Cancellation,
    host_authority: Option<ColdOfflineFlacAuthority>,
) -> Result<(StereoPcmReader, StereoPcmControl, Arc<Shared>)> {
    authority.check()?;
    if !(8000..=384000).contains(&rate) || first >= end {
        return Err(Error::InvalidInput("invalid stereo PCM source range"));
    }
    // mpv's rawaudio demuxer reads rate/8 frames per packet. Startup must
    // admit that real first packet plus the linked limiter's first quantum.
    let startup = u64::from(rate.div_ceil(8)) + QUANTUM as u64;
    let lead = u64::from(rate.div_ceil(4));
    let capacity = (startup.max(lead) + 2 * QUANTUM as u64) as usize;
    let mut pcm = VecDeque::new();
    pcm.try_reserve_exact(capacity * 2)
        .map_err(|_| Error::Media("paced stereo PCM allocation failed"))?;
    let mut controls = VecDeque::new();
    controls
        .try_reserve_exact(MAX_CONTROLS)
        .map_err(|_| Error::Media("stereo control allocation failed"))?;
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            pcm,
            decoded: first,
            produced: 0,
            delivered: 0,
            native: 0,
            entry: None,
            paused: true,
            seeking: false,
            end: false,
            failed: false,
            controls,
            requested: 0,
            applied: 0,
        }),
        wake: Condvar::new(),
        cancelled: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        reader_closed: AtomicBool::new(false),
        sink_retired: AtomicBool::new(true),
        authority,
        host_authority,
        rate,
        first,
        end,
        capacity,
        startup,
        lead,
    });
    let control = StereoPcmControl(shared.clone());
    Ok((StereoPcmReader::new(shared.clone()), control, shared))
}

impl Shared {
    pub(crate) fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(Error::Media("stereo PCM producer failed"));
        }
        if let Err(error) = self.authority.check() {
            self.cancelled.store(true, Ordering::Release);
            self.wake.notify_all();
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn authority_clone(self: &Arc<Self>) -> Cancellation {
        let shared = self.clone();
        Cancellation::new(move || shared.check().is_ok())
    }

    /// Apply every accepted control in order, even while no audio credit is
    /// available. No graph mutation happens on libmpv's callback/actor thread.
    pub(crate) fn wait_for_credit(&self, graph: &mut LocalStereoSubgraph) -> Result<()> {
        loop {
            self.check()?;
            let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
            if state.failed || state.end {
                return Err(Error::State("stereo PCM producer is terminal"));
            }
            if let Some((revision, plan)) = state.controls.pop_front() {
                drop(state);
                graph.configure(plan)?;
                let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
                state.applied = revision;
                self.wake.notify_all();
                continue;
            }
            let credit = if state.entry.is_none() {
                self.startup
            } else if state.paused || state.seeking {
                state.produced
            } else {
                state.native.saturating_add(self.lead)
            };
            if state.produced < credit && state.pcm.len() / 2 + QUANTUM <= self.capacity {
                return Ok(());
            }
            let _guard = self
                .wake
                .wait_timeout(state, WAIT)
                .map_err(|_| Error::LockPoisoned)?;
        }
    }

    pub(crate) fn publish(&self, samples: &[f32], decoded: u64, end: bool) -> Result<()> {
        self.check()?;
        if !samples.len().is_multiple_of(2)
            || samples.len() / 2 > QUANTUM
            || samples.iter().any(|sample| !sample.is_finite())
        {
            return Err(Error::Media("invalid paced stereo output"));
        }
        let mut state = self.state.lock().map_err(|_| Error::LockPoisoned)?;
        self.check()?;
        if state.failed
            || state.end
            || decoded < state.decoded
            || decoded > self.end
            || (state.pcm.len() + samples.len()) / 2 > self.capacity
        {
            return Err(Error::State("invalid paced stereo publication"));
        }
        state.pcm.extend(samples);
        state.produced = state
            .produced
            .checked_add((samples.len() / 2) as u64)
            .ok_or(Error::State("stereo output clock exhausted"))?;
        state.decoded = decoded;
        state.end = end;
        self.wake.notify_all();
        Ok(())
    }

    pub(crate) fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        if let Ok(mut state) = self.state.lock() {
            state.failed = true;
            state.pcm.clear();
        }
        self.wake.notify_all();
    }
}

impl StereoPcmControl {
    /// Nonblocking cancellation for the native cancel callback. It never
    /// changes the account grant and does not require the read mutex.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.wake.notify_all();
    }

    /// Accepted revision, not a claim of audible application. The worker's
    /// applied_revision acknowledges the real same-instance configuration.
    pub fn configure(&self, plan: LocalStereoMaterialPlan) -> Result<u64> {
        self.0.check()?;
        let mut state = self.0.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.failed || state.end || state.controls.len() == MAX_CONTROLS {
            return Err(Error::State("stereo controls unavailable"));
        }
        let revision = state
            .requested
            .checked_add(1)
            .ok_or(Error::State("stereo control revision exhausted"))?;
        state.controls.push_back((revision, plan));
        state.requested = revision;
        self.0.wake.notify_all();
        Ok(revision)
    }

    /// Blocking native-worker acknowledgement, never call on a UI thread.
    /// It proves graph configuration, not that buffered PCM is already heard.
    pub fn wait_applied(&self, revision: u64, timeout: Duration) -> Result<()> {
        if timeout.is_zero() || timeout > Duration::from_secs(3) {
            return Err(Error::InvalidInput("invalid stereo control wait"));
        }
        let deadline = Instant::now() + timeout;
        loop {
            self.0.check()?;
            let state = self.0.state.lock().map_err(|_| Error::LockPoisoned)?;
            if revision > state.requested {
                return Err(Error::InvalidInput("unknown stereo control revision"));
            }
            if state.applied >= revision {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if state.end || remaining.is_zero() {
                return Err(Error::State("stereo control acknowledgement timed out"));
            }
            let _guard = self
                .0
                .wake
                .wait_timeout(state, remaining.min(WAIT))
                .map_err(|_| Error::LockPoisoned)?;
        }
    }

    pub fn snapshot(&self) -> Result<StereoPcmSnapshot> {
        let state = self.0.state.lock().map_err(|_| Error::LockPoisoned)?;
        Ok(StereoPcmSnapshot {
            first_source_frame: self.0.first,
            source_end: self.0.end,
            decoded_source_frame: state.decoded,
            produced_frames: state.produced,
            delivered_frames: state.delivered,
            buffered_frames: (state.pcm.len() / 2) as u32,
            native_output_frame: state.native,
            native_source_frame: self.0.end.min(self.0.first.saturating_add(state.native)),
            native_entry: state.entry,
            paused: state.paused,
            seeking: state.seeking,
            graph_end: state.end,
            requested_revision: state.requested,
            applied_revision: state.applied,
            cancelled: self.0.cancelled.load(Ordering::Acquire),
            failed: self.0.failed.load(Ordering::Acquire),
            reader_closed: self.0.reader_closed.load(Ordering::Acquire),
            native_sink_retired: self.0.sink_retired.load(Ordering::Acquire),
        })
    }

    pub(crate) fn rate(&self) -> u32 {
        self.0.rate
    }

    pub(crate) fn authority(&self) -> Cancellation {
        let control = self.clone();
        Cancellation::new(move || control.0.check().is_ok())
    }

    pub(crate) fn validate_host(&self) -> Result<()> {
        self.0.check()?;
        if let Some(authority) = &self.0.host_authority {
            authority.validate()?;
        }
        self.0.check()
    }

    pub(crate) fn bind_entry(&self, entry: i64) -> Result<()> {
        self.validate_host()?;
        let mut state = self.0.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.entry.is_some() {
            return Err(Error::State("stereo PCM entry is already bound"));
        }
        state.entry = Some(entry);
        state.paused = true;
        self.0.wake.notify_all();
        Ok(())
    }

    pub(crate) fn publish_sink(&self) -> Result<()> {
        self.validate_host()?;
        if self
            .0
            .sink_retired
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::State("stereo native sink is already published"));
        }
        Ok(())
    }

    /// Native owner only: exact END_FILE EOF/Stop, or synchronous native
    /// destruction. A stream close alone does not acknowledge queued AO audio.
    pub(crate) fn retire_sink(&self) {
        self.0.sink_retired.store(true, Ordering::Release);
        self.0.wake.notify_all();
    }

    /// Only the native engine feeds this observation. There is no renderer
    /// IPC or decoder/read-counter route to grant host playback credit.
    pub(crate) fn observe(
        &self,
        entry: i64,
        position: Option<f64>,
        paused: bool,
        seeking: bool,
    ) -> Result<()> {
        self.validate_host()?;
        let mut state = self.0.state.lock().map_err(|_| Error::LockPoisoned)?;
        if state.entry != Some(entry) {
            return Err(Error::StaleResponse);
        }
        state.paused = paused;
        state.seeking = seeking;
        if !seeking
            && let Some(position) = position.filter(|value| value.is_finite() && *value >= 0.0)
        {
            let frames = (position * f64::from(self.0.rate)).floor();
            // A clock cannot acknowledge audio which this reader has never
            // delivered. Do not convert inconsistent/native-seek values into
            // new producer credit.
            if frames <= state.delivered as f64 {
                state.native = state.native.max(frames as u64);
            }
        }
        self.0.wake.notify_all();
        Ok(())
    }
}
