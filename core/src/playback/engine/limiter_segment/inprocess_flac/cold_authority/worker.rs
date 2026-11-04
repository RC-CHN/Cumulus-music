//! Keep the sole authorized reader and libFLAC handle on one worker.
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
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq, Eq)]
struct PassId {
    logical: Generation,
    decoder: Generation,
}

type Reply<T> = SyncSender<io::Result<T>>;

enum Request {
    Start {
        logical: Generation,
        first: u64,
        gate: Cancellation,
        reply: Reply<PassId>,
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
    thread: Option<JoinHandle<()>>,
    pub counters: Arc<Counters>,
}

impl Worker {
    pub(super) fn open(reader: ColdOfflineFlacReader, total: u64) -> io::Result<Self> {
        let (send, requests) = sync_channel(1);
        let (ready, status) = sync_channel(1);
        let counters = Arc::new(Counters::default());
        let trace = counters.clone();
        let thread = thread::Builder::new()
            .name("cold-flac-probe".into())
            .spawn(move || run(reader, total, requests, ready, &trace))?;
        let result = Self {
            send: Some(send),
            thread: Some(thread),
            counters,
        };
        status.recv_timeout(WAIT).map_err(io::Error::other)??;
        Ok(result)
    }

    pub(super) fn factory(&self, gate: Cancellation) -> StartPass {
        let send = self.send.as_ref().unwrap().clone();
        Box::new(move |logical, first| {
            let id = request(&send, &gate, |reply| Request::Start {
                logical,
                first,
                gate: gate.clone(),
                reply,
            })?;
            let send = send.clone();
            let gate = gate.clone();
            Ok(Box::new(move || {
                request(&send, &gate, |reply| Request::Pull { id, reply }).map_err(Error::from)
            }))
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The test owns Worker longer than the sink. Once the sink closes,
        // there are no callback senders; dropping ours closes the worker.
        self.send.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
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
    first: u64,
    gate: Cancellation,
}

fn run(
    reader: ColdOfflineFlacReader,
    total: u64,
    requests: Receiver<Request>,
    ready: Reply<()>,
    counters: &Counters,
) {
    let api = match Api::open(&sdk_path()) {
        Ok(api) => api,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let source = reader.authority().cancellation();
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
    let mut epoch = 1_u64;
    while let Ok(request) = requests.recv() {
        match request {
            Request::Start {
                logical,
                first,
                gate,
                reply,
            } => {
                let result = (|| {
                    gate.check().map_err(io::Error::other)?;
                    if active.as_ref().is_some_and(|pass| {
                        logical.0 < pass.id.logical.0
                            || (logical == pass.id.logical && first != pass.first)
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
                    decoder.seek(id.decoder, first).map_err(|error| {
                        let after = decoder.source().stats;
                        io::Error::other(format!(
                            "FLAC seek to {first}, {} bytes and {} seeks: {error}",
                            after.bytes_read - before.bytes_read,
                            after.seeks - before.seeks
                        ))
                    })?;
                    gate.check().map_err(io::Error::other)?;
                    let after = decoder.source().stats;
                    counters
                        .byte_seeks
                        .fetch_add(after.seeks - before.seeks, Ordering::Release);
                    counters
                        .max_seek_bytes
                        .fetch_max(after.bytes_read - before.bytes_read, Ordering::Release);
                    counters.passes.fetch_add(1, Ordering::Release);
                    active = Some(Pass { id, first, gate });
                    Ok(id)
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
