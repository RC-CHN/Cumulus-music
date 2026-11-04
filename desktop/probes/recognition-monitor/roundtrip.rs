// The only transport is a fixture held in this process. No production HTTP,
// local listening socket, account, or origin configuration is used.
use crate::pulse::{ProbeError::Rejected, Result};
use cumulus_core::{
    protocol::wire::eapi,
    song_recognition::{
        FingerprintLogMode, FingerprintPcm8k, centered_default_spectrum, compress_packed_packet,
        pack_default_packet,
        request::{
            Event, FixtureRequest, Generation, RequestContext, ResultStatus, Scheduler, SessionUuid,
        },
        seal_compressed_packet,
    },
};
use std::time::Instant;

const UUID: &str = "3DB43855-B72D-46D8-ACAD-0BA047E0CF13";
const MISS: &str = r#"{"code":200,"data":{"type":0,"result":[]}}"#;
const HIT: &str =
    r#"{"code":200,"data":{"type":1,"result":[{"song":{"id":7001}},{"song":{"id":"7002"}}]}}"#;

pub struct RoundTrip {
    scheduler: Scheduler,
    generation: Generation,
    clock: Instant,
    transport: Vec<FixtureRequest>,
    peak_counts: Vec<usize>,
    matched: bool,
    cancel_in_flight: bool,
}

impl RoundTrip {
    pub fn new(cancel_in_flight: bool) -> Result<Self> {
        let mut scheduler = Scheduler::default();
        let session =
            SessionUuid::from_host(UUID).map_err(|_| Rejected("fixture UUID rejected"))?;
        let clock = Instant::now();
        let generation = scheduler
            .start(0, session)
            .map_err(|_| Rejected("fixture scheduler did not start"))?;
        Ok(Self {
            scheduler,
            generation,
            clock,
            transport: Vec::new(),
            peak_counts: Vec::new(),
            matched: false,
            cancel_in_flight,
        })
    }

    fn now(&self) -> u64 {
        // The standalone fixture's wall clock is bounded by capture and its runner.
        self.clock.elapsed().as_millis() as u64
    }

    /// Returns true when the record stream should be released: either because
    /// recognition matched or because cancellation has an actual request to fence.
    pub fn push(&mut self, samples: &[i16]) -> Result<bool> {
        if !matches!(
            self.scheduler.push_pcm(self.generation, self.now(), samples),
            Ok(Event::Ready(n)) if n == samples.len()
        ) {
            return Err(Rejected("fixture scheduler lost converted samples"));
        }
        let job = match self.scheduler.next_prefix(self.generation, self.now()) {
            Ok(Event::Waiting) => return Ok(false),
            Ok(Event::Ready(job)) => job,
            _ => return Err(Rejected("fixture scheduler did not reserve a prefix")),
        };
        let times = self.transport.len() + 1;
        if times > 2 || job.samples().len() != times * 24_000 {
            return Err(Rejected("fixture cumulative recognition threshold changed"));
        }
        let pcm = FingerprintPcm8k::new(job.samples())
            .map_err(|_| Rejected("invalid fixture fingerprint PCM"))?;
        let spectrum = centered_default_spectrum(&pcm, FingerprintLogMode::Sse)
            .ok_or(Rejected("fixture fingerprint spectrum is absent"))?;
        let peaks = spectrum.peaks();
        if peaks.is_empty() {
            return Err(Rejected("synthetic waveform produced no recognition peaks"));
        }
        let packed = pack_default_packet(job.samples().len() as u32, &peaks)
            .map_err(|_| Rejected("fixture fingerprint packing failed"))?;
        let compressed = compress_packed_packet(&packed)
            .map_err(|_| Rejected("fixture stock compression failed"))?
            .ok_or(Rejected("fixture compressed fingerprint is absent"))?;
        let fingerprint = seal_compressed_packet(&compressed)
            .map_err(|_| Rejected("fixture fingerprint sealing failed"))?
            .ok_or(Rejected("fixture sealed fingerprint is absent"))?;
        let context = RequestContext::new("3.1.42.205594", "synthetic-device", "synthetic-sign")
            .map_err(|_| Rejected("fixture request context rejected"))?;
        let receipt = job.receipt();
        let tick = u32::MAX.wrapping_add((times - 1) as u32);
        let Ok(Event::Ready(request)) =
            self.scheduler
                .prepare(receipt, self.now(), &fingerprint, &context, tick)
        else {
            return Err(Rejected(
                "fixture scheduler did not prepare its one-shot request",
            ));
        };
        let params = request
            .encrypted_form()
            .strip_prefix("params=")
            .ok_or(Rejected("fixture request form changed"))?;
        let (path, body) = eapi::unwrap(params)
            .map_err(|_| Rejected("fixture request EAPI verification failed"))?;
        if path != "/api/music/audio/match"
            || body["times"].as_u64() != Some(times as u64)
            || body["duration"].as_u64() != Some(3 * times as u64)
            || body["sessionId"].as_str() != Some(UUID)
            || body["header"]["requestId"].as_str() != Some(tick.to_string().as_str())
            || body["rawdata"].as_str() != Some(fingerprint.rawdata().as_str())
            || request.receipt() != receipt
        {
            return Err(Rejected(
                "fixture request payload does not match the captured prefix",
            ));
        }
        self.transport.push(request);
        self.peak_counts.push(peaks.len());
        if self.cancel_in_flight {
            return Ok(true);
        }
        if times == 1 {
            if !matches!(
                self.scheduler.receive(receipt, self.now(), Some(200), MISS),
                Ok(Event::Waiting)
            ) {
                return Err(Rejected("fixture first miss prematurely ended recognition"));
            }
            return Ok(false);
        }
        let Ok(Event::Completed(result)) =
            self.scheduler.receive(receipt, self.now(), Some(0), HIT)
        else {
            return Err(Rejected("fixture match did not end recognition"));
        };
        if result.times != 2
            || result.code != Some(0)
            || !matches!(
                result.status(),
                ResultStatus::Matched { song_ids }
                    if song_ids.iter().map(String::as_str).eq(["7001", "7002"])
            )
            || !matches!(
                self.scheduler
                    .receive(self.transport[0].receipt(), self.now(), Some(200), HIT),
                Ok(Event::Ignored)
            )
        {
            return Err(Rejected(
                "fixture result precedence or old callback fence changed",
            ));
        }
        self.matched = true;
        Ok(true)
    }

    pub fn verify_cancelled(&mut self) -> Result<()> {
        if self.transport.len() != 1 || self.matched {
            return Err(Rejected(
                "cancel did not leave exactly one encoded request in flight",
            ));
        }
        let Event::Completed(outcome) = self.scheduler.cancel(self.generation) else {
            return Err(Rejected("fixture scheduler did not cancel"));
        };
        if outcome.times != 1
            || outcome.status() != &ResultStatus::Cancelled
            || !matches!(
                self.scheduler
                    .receive(self.transport[0].receipt(), self.now(), Some(200), HIT),
                Ok(Event::Ignored)
            )
        {
            return Err(Rejected(
                "fixture cancellation did not reject its late match",
            ));
        }
        Ok(())
    }

    pub fn verify_matched(&self) -> Result<()> {
        if !self.matched || self.transport.len() != 2 {
            return Err(Rejected(
                "capture did not complete two-prefix fixture recognition",
            ));
        }
        Ok(())
    }

    pub fn peak_counts(&self) -> &[usize] {
        &self.peak_counts
    }
}
