use super::{
    Admission, BLOCK, Cache, Fill, Key, Lease,
    range::{Chunk, RangeMedia},
    store::DecoderAcceptance,
};
use crate::{Error, Result, download::transfer::MediaUrl, playback::engine::Cancellation};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc as async_mpsc, task::JoinHandle};

const RANGE_TIMEOUT: Duration = Duration::from_secs(4);

/// Native-only, generation-bound representation. Its constructors and
/// request machinery are private; renderer and plugins receive no media URL.
pub struct StreamingAudioReader {
    input: Input,
    authority: Cancellation,
    position: u64,
    size: u64,
}

enum Input {
    Hit(Lease),
    Fill(Fill, RangeWorker),
}

struct Job {
    offset: u64,
    length: usize,
    reply: mpsc::SyncSender<Result<Chunk>>,
}

struct RangeWorker {
    jobs: async_mpsc::Sender<Job>,
    task: JoinHandle<()>,
}
impl RangeWorker {
    fn start(
        backend: Arc<dyn RangeMedia>,
        url: MediaUrl,
        size: u64,
        etag: String,
        authority: Cancellation,
    ) -> Self {
        let (jobs, mut requests) = async_mpsc::channel::<Job>(1);
        let task = tokio::spawn(async move {
            while let Some(job) = requests.recv().await {
                if authority.is_cancelled() {
                    break;
                }
                let result = tokio::time::timeout(
                    RANGE_TIMEOUT,
                    backend.get(&url, job.offset, job.length, size, Some(&etag)),
                )
                .await
                .map_err(|_| Error::Transport("streaming media request timed out".into()))
                .and_then(|value| value)
                .and_then(|chunk| {
                    authority.check()?;
                    if chunk.etag != etag || chunk.bytes.len() != job.length {
                        return Err(Error::Response("streaming media representation changed"));
                    }
                    Ok(chunk)
                });
                if job.reply.send(result).is_err() {
                    break;
                }
            }
        });
        Self { jobs, task }
    }

    fn get(&self, offset: u64, length: usize, authority: &Cancellation) -> Result<Chunk> {
        let (reply, response) = mpsc::sync_channel(1);
        self.jobs
            .try_send(Job {
                offset,
                length,
                reply,
            })
            .map_err(|_| Error::Cancelled)?;
        let deadline = Instant::now() + RANGE_TIMEOUT;
        loop {
            authority.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Transport("streaming media worker timed out".into()));
            }
            match response.recv_timeout(remaining.min(Duration::from_millis(25))) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Cancelled),
            }
        }
    }
}
impl Drop for RangeWorker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl StreamingAudioReader {
    pub(crate) async fn prepare(
        cache: Arc<Cache>,
        backend: Arc<dyn RangeMedia>,
        revision: u64,
        key: Key,
        url: MediaUrl,
        authority: Cancellation,
    ) -> Result<Option<Self>> {
        let size = key.size();
        let validation = authority.clone();
        let admission = tokio::task::spawn_blocking(move || cache.admit(revision, key, validation))
            .await
            .map_err(|_| Error::State("streaming media admission worker stopped"))??;
        let input = match admission {
            Some(Admission::Hit(lease)) => Input::Hit(lease),
            Some(Admission::Fill(mut fill)) => {
                // Qualify the actual native source before replacing the stock
                // URL path. No background fetch ahead of the first block.
                let length = size.min(BLOCK as u64) as usize;
                let chunk =
                    tokio::time::timeout(RANGE_TIMEOUT, backend.get(&url, 0, length, size, None))
                        .await
                        .map_err(|_| {
                            Error::Transport("streaming media preflight timed out".into())
                        })??;
                authority.check()?;
                if chunk.bytes.len() != length || !mp3(&chunk.bytes) {
                    return Ok(None);
                }
                let etag = chunk.etag;
                fill = tokio::task::spawn_blocking(move || {
                    fill.write_block(0, &chunk.bytes)?;
                    Ok::<_, Error>(fill)
                })
                .await
                .map_err(|_| Error::State("streaming media preflight worker stopped"))??;
                Input::Fill(
                    fill,
                    RangeWorker::start(backend, url, size, etag, authority.clone()),
                )
            }
            None => return Ok(None),
        };
        authority.check()?;
        Ok(Some(Self {
            input,
            authority,
            position: 0,
            size,
        }))
    }

    pub(crate) fn decoder_acceptance(&self) -> Option<DecoderAcceptance> {
        match &self.input {
            Input::Fill(fill, _) => Some(fill.decoder_acceptance()),
            Input::Hit(_) => None,
        }
    }

    pub(crate) fn authority(&self) -> Cancellation {
        self.authority.clone()
    }

    pub(crate) fn restrict(&mut self, additional: Cancellation) {
        let original = self.authority.clone();
        self.authority =
            Cancellation::new(move || !original.is_cancelled() && !additional.is_cancelled());
    }

    fn read_bytes(&mut self, output: &mut [u8]) -> Result<usize> {
        self.authority.check()?;
        let count = output
            .len()
            .min((self.size - self.position) as usize)
            .min(BLOCK);
        if count == 0 {
            return Ok(0);
        }
        let offset = self.position;
        let count = match &mut self.input {
            Input::Hit(lease) => {
                lease.read_at(offset, &mut output[..count])?;
                count
            }
            Input::Fill(fill, worker) => {
                let block = offset / BLOCK as u64 * BLOCK as u64;
                let length = (self.size - block).min(BLOCK as u64) as usize;
                let count = count.min(length - (offset - block) as usize);
                if !fill.available(block) {
                    let chunk = worker.get(block, length, &self.authority)?;
                    fill.write_block(block, &chunk.bytes)?;
                }
                fill.publish_if_complete()?;
                fill.read_at(offset, &mut output[..count])?;
                count
            }
        };
        self.authority.check()?;
        self.position += count as u64;
        Ok(count)
    }
}

fn mp3(bytes: &[u8]) -> bool {
    matches!(bytes, [b'I', b'D', b'3', ..])
        || matches!(bytes, [0xff, second, ..] if second & 0xe0 == 0xe0 && second & 0x06 != 0)
}

impl Read for StreamingAudioReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.read_bytes(output)
            .inspect_err(|_| self.authority.cancel())
            .map_err(io::Error::other)
    }
}
impl Seek for StreamingAudioReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.authority.check().map_err(io::Error::other)?;
        let next = match to {
            SeekFrom::Start(pos) => Some(pos),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
        }
        .filter(|pos| *pos <= self.size)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid streaming media seek")
        })?;
        self.position = next;
        Ok(next)
    }
}
