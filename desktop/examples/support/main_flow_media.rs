//! Fixed FLAC with a durable pause handshake. Never opens a media socket.
use cumulus_core::{
    Error,
    download::transfer::{MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const DATA: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.flac");
pub const URL: &str = "https://m1.music.126.net/cumulus-main-flow-fixture.flac";
pub const PREFIX: usize = 4096;
pub const MD5: &str = "6b1c7d8698e1e941ad6f2046faa0b611";
const ETAG: &str = "\"synthetic-main-flow-flac\"";

pub struct Media {
    store: Arc<StateStore>,
    waiting: Arc<AtomicBool>,
    cold_resume: bool,
    offsets: Mutex<Vec<(usize, bool)>>,
}
impl Media {
    pub fn new(store: Arc<StateStore>, cold_resume: bool) -> Self {
        Self {
            store,
            waiting: Default::default(),
            cold_resume,
            offsets: Default::default(),
        }
    }
    pub fn prefix_waiting(&self) -> bool {
        self.waiting.load(Ordering::Acquire)
    }
    pub fn facts(&self) -> Value {
        let requests = self.offsets.lock().unwrap();
        json!({"count":requests.len(),"requests":*requests})
    }
    pub fn get(&self, request: MediaRequest) -> cumulus_core::Result<MediaReply> {
        let mut requests = self.offsets.lock().unwrap();
        // A new authorization with identical size and MD5 preserves the
        // prefix, but deliberately clears the old response validator.
        let resumed = request.offset == PREFIX as u64 && request.if_range.is_none();
        let first = request.offset == 0 && request.if_range.is_none();
        let valid_sequence = if self.cold_resume {
            requests.is_empty() && resumed
        } else {
            matches!(
                (requests.len(), first, resumed),
                (0, true, false) | (1, false, true)
            )
        };
        if request.url.as_str() != URL || !valid_sequence {
            return Err(Error::State("unexpected synthetic media request"));
        }
        let offset = if first { 0 } else { PREFIX };
        requests.push((offset, request.if_range.is_some()));
        let mut headers = BTreeMap::from([
            ("content-length".into(), (DATA.len() - offset).to_string()),
            ("etag".into(), ETAG.into()),
        ]);
        if resumed {
            headers.insert(
                "content-range".into(),
                format!("bytes {offset}-{}/{}", DATA.len() - 1, DATA.len()),
            );
        }
        Ok(MediaReply {
            status: if first { 200 } else { 206 },
            headers,
            body: Box::new(Body {
                offset,
                first,
                store: self.store.clone(),
                waiting: self.waiting.clone(),
            }),
        })
    }
}
struct Body {
    offset: usize,
    first: bool,
    store: Arc<StateStore>,
    waiting: Arc<AtomicBool>,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            if self.first && self.offset == PREFIX {
                // The transfer has accepted the first read before it can
                // request another. Release this read only after real IPC has
                // persisted Pause; the worker must discard the returned bytes.
                self.waiting.store(true, Ordering::Release);
                tokio::time::timeout(Duration::from_secs(25), async {
                    loop {
                        if self
                            .store
                            .values("2", "download_controls_v1")?
                            .values()
                            .any(|v| v == "pause")
                        {
                            return Ok::<(), Error>(());
                        }
                        tokio::time::sleep(Duration::from_millis(15)).await;
                    }
                })
                .await
                .map_err(|_| Error::State("synthetic pause directive did not arrive"))??;
            }
            let limit = if self.first && self.offset < PREFIX {
                PREFIX
            } else {
                DATA.len()
            };
            let size = output.len().min(limit - self.offset);
            output[..size].copy_from_slice(&DATA[self.offset..self.offset + size]);
            self.offset += size;
            Ok(size)
        })
    }
}
