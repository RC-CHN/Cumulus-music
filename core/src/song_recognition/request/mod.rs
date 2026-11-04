//! Offline PC 3.1.42.205594 recognition request scheduling and wire shape.
//!
//! This has no service transport, account state, capture, or desktop caller.
//! Time is injected in monotonic milliseconds. The consumer owns each one-shot
//! prepared request; repository tests use only an in-memory transport.

mod wire;
pub use wire::{
    CANONICAL_PATH, DEFAULT_POST_URL, FixtureRequest, OS, REFERER, RequestContext, SessionUuid,
    USER_AGENT,
};

use super::SealedFingerprint;
use crate::{Error, Result};
use uuid::Uuid;
use wire::InterpretedResponse;

pub const PCM_RATE: usize = 8_000;
pub const MAX_ATTEMPTS: u8 = 5;
pub const TIMEOUT_MS: u64 = 16_000;
const PREFIX_SECONDS: u8 = 3;
const MAX_PCM: usize = MAX_ATTEMPTS as usize * PREFIX_SECONDS as usize * PCM_RATE;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Generation {
    sequence: u64,
    nonce: Uuid,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Receipt {
    generation: Generation,
    times: u8,
}

impl Receipt {
    pub fn times(self) -> u8 {
        self.times
    }

    pub fn duration_seconds(self) -> u8 {
        self.times * PREFIX_SECONDS
    }

    fn index(self) -> usize {
        usize::from(self.times - 1)
    }
}

/// Cumulative 8 kHz mono input. No formatting or serialization of the samples.
pub struct PrefixJob {
    receipt: Receipt,
    pcm: Vec<i16>,
}

impl PrefixJob {
    pub fn receipt(&self) -> Receipt {
        self.receipt
    }

    pub fn samples(&self) -> &[i16] {
        &self.pcm
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ResultStatus {
    Matched { song_ids: Vec<u64> },
    NoTrack,
    Timeout,
    Offline,
    Unknown,
    Cancelled,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Completed {
    /// Native callback code when available. The local deadline uses -5.
    pub code: Option<i32>,
    pub times: u8,
    pub status: ResultStatus,
}

/// Every event yields either one value or a terminal transition. Closing a
/// generation discards its retained PCM and revokes all queued receipts.
pub enum Event<T> {
    Ready(T),
    Waiting,
    Ignored,
    Completed(Completed),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum JobState {
    Unissued,
    Fingerprinting,
    AwaitingResponse,
    Consumed,
}

struct Active {
    generation: Generation,
    uuid: SessionUuid,
    elapsed_at: u64,
    deadline: u64,
    pcm: Vec<i16>,
    issued: u8,
    jobs: [JobState; MAX_ATTEMPTS as usize],
}

impl Active {
    fn receipt(&self, times: u8) -> Receipt {
        Receipt {
            generation: self.generation,
            times,
        }
    }
}

/// Pure recognition lifetime. Start again for a new capture or account
/// generation. Calls from previous generations are ignored before inspecting
/// their data or time. It never opens a device or sends a request.
#[derive(Default)]
pub struct Scheduler {
    sequence: u64,
    active: Option<Active>,
}

impl Scheduler {
    /// The host supplies the UUID once for the native session. The local
    /// generation nonce is distinct, even if a fixture repeats its wire UUID.
    pub fn start(&mut self, now_ms: u64, uuid: SessionUuid) -> Result<Generation> {
        let deadline = now_ms
            .checked_add(TIMEOUT_MS)
            .ok_or(Error::InvalidInput("recognition clock overflow"))?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Error::State("recognition generation exhausted"))?;
        let generation = Generation {
            sequence,
            nonce: Uuid::new_v4(),
        };
        self.sequence = sequence;
        self.active = Some(Active {
            generation,
            uuid,
            elapsed_at: now_ms,
            deadline,
            pcm: Vec::new(),
            issued: 0,
            jobs: [JobState::Unissued; MAX_ATTEMPTS as usize],
        });
        Ok(generation)
    }

    /// Feed only already converted 8 kHz mono PCM for this generation.
    /// Samples after the final 15-second prefix are discarded.
    pub fn push_pcm(
        &mut self,
        generation: Generation,
        now_ms: u64,
        samples: &[i16],
    ) -> Result<Event<usize>> {
        let active = match self.check_time(generation, now_ms)? {
            Event::Ready(active) => active,
            other => return Ok(without_ready(other)),
        };
        let count = samples.len().min(MAX_PCM - active.pcm.len());
        active.pcm.extend_from_slice(&samples[..count]);
        Ok(Event::Ready(count))
    }

    /// Reserve the next threshold once, irrespective of how large the latest
    /// capture packet was. Multiple distinct prefix jobs may await a response.
    pub fn next_prefix(&mut self, generation: Generation, now_ms: u64) -> Result<Event<PrefixJob>> {
        let active = match self.check_time(generation, now_ms)? {
            Event::Ready(active) => active,
            other => return Ok(without_ready(other)),
        };
        if active.issued == MAX_ATTEMPTS {
            return Ok(Event::Waiting);
        }
        let receipt = active.receipt(active.issued + 1);
        let samples = usize::from(receipt.duration_seconds()) * PCM_RATE;
        if active.pcm.len() < samples {
            return Ok(Event::Waiting);
        }
        active.issued += 1;
        active.jobs[receipt.index()] = JobState::Fingerprinting;
        Ok(Event::Ready(PrefixJob {
            receipt,
            pcm: active.pcm[..samples].to_vec(),
        }))
    }

    /// Convert an outstanding job and a sealed fingerprint to a single
    /// inspectable EAPI fixture. No Cookie, connection, or retry is added.
    pub fn prepare(
        &mut self,
        receipt: Receipt,
        now_ms: u64,
        fingerprint: &SealedFingerprint,
        context: &RequestContext,
        tick_count: u32,
    ) -> Result<Event<FixtureRequest>> {
        if !self.has_job(receipt, JobState::Fingerprinting) {
            return Ok(Event::Ignored);
        }
        let active = match self.check_time(receipt.generation, now_ms)? {
            Event::Ready(active) => active,
            other => return Ok(without_ready(other)),
        };
        let request =
            FixtureRequest::encode(receipt, &active.uuid, fingerprint, context, tick_count)?;
        active.jobs[receipt.index()] = JobState::AwaitingResponse;
        Ok(Event::Ready(request))
    }

    /// Native 31406D empty-fingerprint result. It has not sent a request.
    pub fn empty_fingerprint(&mut self, receipt: Receipt, now_ms: u64) -> Result<Event<()>> {
        self.accept(receipt, now_ms, JobState::Fingerprinting, Some(-6), "")
    }

    /// Apply an already-decoded synthetic native callback. The bridge status
    /// code and the JSON body's code are intentionally distinct.
    pub fn receive(
        &mut self,
        receipt: Receipt,
        now_ms: u64,
        bridge_code: Option<i32>,
        httpmsg: &str,
    ) -> Result<Event<()>> {
        self.accept(
            receipt,
            now_ms,
            JobState::AwaitingResponse,
            bridge_code,
            httpmsg,
        )
    }

    fn accept(
        &mut self,
        receipt: Receipt,
        now_ms: u64,
        state: JobState,
        bridge_code: Option<i32>,
        httpmsg: &str,
    ) -> Result<Event<()>> {
        if !self.has_job(receipt, state) {
            return Ok(Event::Ignored);
        }
        let active = match self.check_time(receipt.generation, now_ms)? {
            Event::Ready(active) => active,
            other => return Ok(without_ready(other)),
        };
        active.jobs[receipt.index()] = JobState::Consumed;
        let response = InterpretedResponse::new(httpmsg);
        // 3078DD + 308060 (native hit), or 307927–307941 (fifth callback).
        if !response.native_hit && receipt.times < MAX_ATTEMPTS {
            return Ok(Event::Waiting);
        }
        let status = match bridge_code {
            Some(0 | 200) if response.song_ids.is_empty() => ResultStatus::NoTrack,
            Some(0 | 200) => ResultStatus::Matched {
                song_ids: response.song_ids,
            },
            Some(code) if code < 0 => ResultStatus::Timeout,
            Some(code) if code > 0 => ResultStatus::Offline,
            _ => ResultStatus::Unknown,
        };
        self.active = None;
        Ok(Event::Completed(Completed {
            code: bridge_code,
            times: receipt.times,
            status,
        }))
    }

    /// Explicit user/host cancellation revokes the generation synchronously.
    /// A stale cancellation cannot close a newer generation.
    pub fn cancel(&mut self, generation: Generation) -> Event<()> {
        let Some(active) = self.active.as_ref().filter(|a| a.generation == generation) else {
            return Event::Ignored;
        };
        let times = active.issued;
        self.active = None;
        Event::Completed(Completed {
            code: None,
            times,
            status: ResultStatus::Cancelled,
        })
    }

    pub fn tick(&mut self, generation: Generation, now_ms: u64) -> Result<Event<()>> {
        Ok(match self.check_time(generation, now_ms)? {
            Event::Ready(_) => Event::Waiting,
            other => without_ready(other),
        })
    }

    fn has_job(&self, receipt: Receipt, state: JobState) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| a.generation == receipt.generation && a.jobs[receipt.index()] == state)
    }

    fn check_time(&mut self, generation: Generation, now_ms: u64) -> Result<Event<&mut Active>> {
        let Some(active) = self.active.as_mut().filter(|a| a.generation == generation) else {
            return Ok(Event::Ignored);
        };
        if now_ms < active.elapsed_at {
            return Err(Error::InvalidInput("recognition clock moved backward"));
        }
        active.elapsed_at = now_ms;
        if now_ms <= active.deadline {
            return Ok(Event::Ready(self.active.as_mut().unwrap()));
        }
        let times = active.issued;
        self.active = None;
        Ok(Event::Completed(Completed {
            code: Some(-5),
            times,
            status: ResultStatus::Timeout,
        }))
    }
}

fn without_ready<T>(event: Event<&mut Active>) -> Event<T> {
    match event {
        Event::Completed(value) => Event::Completed(value),
        Event::Ignored => Event::Ignored,
        Event::Waiting => Event::Waiting,
        Event::Ready(_) => unreachable!("only non-ready transitions may be propagated"),
    }
}

#[cfg(test)]
mod tests;
