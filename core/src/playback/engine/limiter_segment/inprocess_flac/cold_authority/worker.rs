//! Keep the sole authorized reader and libFLAC handle on one worker.
use super::super::{BYTE_BUDGET, ReadBudget};
use super::{Api, Decoder, Generation, sdk_path};
use crate::{
    Error,
    download::offline::ColdOfflineFlacReader,
    playback::engine::{
        Cancellation,
        limiter_segment::{DecodedPacket, probe_wav::StartPass},
    },
};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(3);
const CLOSE_POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct PassId {
    logical: Generation,
    decoder: Generation,
}

type Reply<T> = SyncSender<io::Result<T>>;

pub(super) enum SeekCheck {
    Ready(PassId),
    ReadBudget { bytes: u64 },
}

enum Request {
    Start {
        logical: Generation,
        first: u64,
        gate: Cancellation,
        reply: Reply<SeekCheck>,
    },
    Pull {
        id: PassId,
        reply: Reply<Option<DecodedPacket>>,
    },
}

#[derive(Default)]
pub(super) struct Counters {
    pub opens: AtomicUsize,
    pub passes: AtomicUsize,
    pub byte_seeks: AtomicUsize,
    pub max_seek_bytes: AtomicU64,
}

pub(super) struct Worker {
    send: Option<SyncSender<Request>>,
    closed: Cancellation,
    retired: Receiver<ColdOfflineFlacReader>,
    thread: Option<JoinHandle<()>>,
    pub counters: Arc<Counters>,
}

impl Worker {
    pub(super) fn open(reader: ColdOfflineFlacReader, total: u64) -> io::Result<Self> {
        let (send, requests) = sync_channel(1);
        let (ready, status) = sync_channel(1);
        let (retire, retired) = sync_channel(1);
        let closed = Cancellation::default();
        let worker_close = closed.clone();
        let counters = Arc::new(Counters::default());
        let trace = counters.clone();
        let thread = thread::Builder::new()
            .name("cold-flac-probe".into())
            .spawn(move || {
                let mut reader = reader;
                run(&mut reader, total, requests, ready, &trace, &worker_close);
                // run has dropped libFLAC and the bounded request receiver.
                // The thread is now the only owner of the original reader.
                let _ = retire.send(reader);
            })?;
        let result = Self {
            send: Some(send),
            closed,
            retired,
            thread: Some(thread),
            counters,
        };
        status.recv_timeout(WAIT).map_err(io::Error::other)??;
        Ok(result)
    }

    pub(super) fn factory(&self, gate: Cancellation) -> StartPass {
        let send = self.send.as_ref().unwrap().clone();
        let gate = self.callback_gate(&gate);
        Box::new(move |logical, first| {
            let id = match start(&send, &gate, logical, first)? {
                SeekCheck::Ready(id) => id,
                SeekCheck::ReadBudget { .. } => return Err(io::Error::other(ReadBudget)),
            };
            let send = send.clone();
            let gate = gate.clone();
            Ok(Box::new(move || {
                request(&send, &gate, |reply| Request::Pull { id, reply }).map_err(Error::from)
            }))
        })
    }

    pub(super) fn try_seek(
        &self,
        logical: Generation,
        first: u64,
        request_gate: &Cancellation,
    ) -> io::Result<SeekCheck> {
        let gate = self.callback_gate(request_gate);
        start(self.send.as_ref().unwrap(), &gate, logical, first)
    }

    fn callback_gate(&self, entry: &Cancellation) -> Cancellation {
        let closed = self.closed.clone();
        let entry = entry.clone();
        Cancellation::new(move || !closed.is_cancelled() && !entry.is_cancelled())
    }

    /// The host calls this only after the old sink stops or is destroyed.
    /// A timeout withholds the reader, even if native I/O returns later.
    pub(super) fn reclaim(mut self) -> io::Result<ColdOfflineFlacReader> {
        self.close();
        let deadline = Instant::now() + WAIT;
        let reader = self
            .retired
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(io::Error::other)?;
        let thread = self.thread.take().unwrap();
        while !thread.is_finished() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::other("cold FLAC worker retirement timed out"));
            }
            thread::park_timeout(remaining.min(Duration::from_millis(1)));
        }
        thread
            .join()
            .map_err(|_| io::Error::other("cold FLAC worker retirement panicked"))?;
        Ok(reader)
    }

    fn close(&mut self) {
        self.closed.cancel();
        self.send.take();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
        // Failure/drop never grants the reader to another sink. The worker
        // closes it itself; a stalled read cannot make this destructor hang.
        if let Some(thread) = self.thread.take().filter(JoinHandle::is_finished) {
            let _ = thread.join();
        }
    }
}

fn start(
    send: &SyncSender<Request>,
    gate: &Cancellation,
    logical: Generation,
    first: u64,
) -> io::Result<SeekCheck> {
    request(send, gate, |reply| Request::Start {
        logical,
        first,
        gate: gate.clone(),
        reply,
    })
}

fn request<T>(
    send: &SyncSender<Request>,
    gate: &Cancellation,
    build: impl FnOnce(Reply<T>) -> Request,
) -> io::Result<T> {
    gate.check().map_err(io::Error::other)?;
    let (reply, output) = sync_channel(1);
    send.try_send(build(reply))
        .map_err(|_| io::Error::other("cold FLAC worker unavailable"))?;
    let result = output.recv_timeout(WAIT).map_err(io::Error::other)?;
    gate.check().map_err(io::Error::other)?;
    result
}

struct Pass {
    id: PassId,
    gate: Cancellation,
}

fn run(
    reader: &mut ColdOfflineFlacReader,
    total: u64,
    requests: Receiver<Request>,
    ready: Reply<()>,
    counters: &Counters,
    closed: &Cancellation,
) {
    let api = match Api::open(&sdk_path()) {
        Ok(api) => api,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let auth = reader.authority().cancellation();
    let transport = closed.clone();
    let source = Cancellation::new(move || !transport.is_cancelled() && !auth.is_cancelled());
    let mut decoder = match Decoder::open(&api, Box::new(reader), Generation(1), source) {
        Ok(decoder) if decoder.total == total => decoder,
        Ok(_) => {
            let _ = ready.send(Err(io::Error::other("unexpected cold FLAC frame count")));
            return;
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    counters.opens.fetch_add(1, Ordering::Release);
    if ready.send(Ok(())).is_err() {
        return;
    }
    let mut active: Option<Pass> = None;
    let mut selected: Option<(Generation, u64)> = None;
    let mut epoch = 1_u64;
    while !closed.is_cancelled() {
        let request = match requests.recv_timeout(CLOSE_POLL) {
            Ok(request) => request,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match request {
            Request::Start {
                logical,
                first,
                gate,
                reply,
            } => {
                let result = (|| {
                    gate.check().map_err(io::Error::other)?;
                    if selected.is_some_and(|(latest, origin)| {
                        logical.0 < latest.0 || (logical == latest && first != origin)
                    }) {
                        return Err(io::Error::other("stale cold FLAC replay"));
                    }
                    epoch = epoch
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("cold FLAC epoch exhausted"))?;
                    let id = PassId {
                        logical,
                        decoder: Generation(epoch),
                    };
                    let before = decoder.source().stats;
                    // A failed attempt invalidates the old decoder pass;
                    // its packet requests can no longer read or publish.
                    selected = Some((logical, first));
                    active = None;
                    if let Err(error) = decoder.seek(id.decoder, first) {
                        gate.check().map_err(io::Error::other)?;
                        let after = decoder.source().stats;
                        let bytes = after.bytes_read - before.bytes_read;
                        if bytes == BYTE_BUDGET as u64
                            && error
                                .get_ref()
                                .is_some_and(|cause| cause.is::<ReadBudget>())
                        {
                            return Ok(SeekCheck::ReadBudget { bytes });
                        }
                        return Err(io::Error::other(format!(
                            "FLAC seek to {first}, {bytes} bytes and {} seeks: {error}",
                            after.seeks - before.seeks
                        )));
                    }
                    gate.check().map_err(io::Error::other)?;
                    let after = decoder.source().stats;
                    counters
                        .byte_seeks
                        .fetch_add(after.seeks - before.seeks, Ordering::Release);
                    counters
                        .max_seek_bytes
                        .fetch_max(after.bytes_read - before.bytes_read, Ordering::Release);
                    counters.passes.fetch_add(1, Ordering::Release);
                    active = Some(Pass { id, gate });
                    Ok(SeekCheck::Ready(id))
                })();
                let _ = reply.send(result);
            }
            Request::Pull { id, reply } => {
                let result = (|| {
                    let pass = active
                        .as_ref()
                        .filter(|pass| pass.id == id)
                        .ok_or_else(|| io::Error::other("stale cold FLAC packet request"))?;
                    pass.gate.check().map_err(io::Error::other)?;
                    let packet = decoder.next().map_err(|error| {
                        io::Error::other(format!("FLAC sequential read: {error}"))
                    })?;
                    pass.gate.check().map_err(io::Error::other)?;
                    packet
                        .map(|mut packet| {
                            if packet.generation != id.decoder {
                                return Err(io::Error::other("wrong native decoder generation"));
                            }
                            packet.generation = id.logical;
                            Ok(packet)
                        })
                        .transpose()
                })();
                let _ = reply.send(result);
            }
        }
    }
}
