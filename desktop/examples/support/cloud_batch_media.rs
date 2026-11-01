//! Fixed synthetic FLAC only; never opens a socket or accepts an arbitrary URL.
use cumulus_core::{
    Error,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use std::sync::atomic::{AtomicUsize, Ordering};
pub const DATA: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.flac");
pub const URL: &str = "https://m1.music.126.net/cumulus-cloud-batch-fixture.flac";
static REQUESTS: AtomicUsize = AtomicUsize::new(0);
pub fn requests() -> usize {
    REQUESTS.load(Ordering::Acquire)
}
pub struct Backend;
impl MediaBackend for Backend {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str() != URL || request.offset >= DATA.len() as u64 {
                return Err(Error::State("cloud batch proof forbids remote media"));
            }
            REQUESTS.fetch_add(1, Ordering::AcqRel);
            let offset = request.offset as usize;
            let mut headers = std::collections::BTreeMap::from([
                ("content-length".into(), (DATA.len() - offset).to_string()),
                ("etag".into(), "\"synthetic-cloud-original\"".into()),
            ]);
            if offset > 0 {
                headers.insert(
                    "content-range".into(),
                    format!("bytes {offset}-{}/{}", DATA.len() - 1, DATA.len()),
                );
            }
            Ok(MediaReply {
                status: if offset == 0 { 200 } else { 206 },
                headers,
                body: Box::new(Body { offset }),
            })
        })
    }
}
struct Body {
    offset: usize,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let size = buffer.len().min(DATA.len() - self.offset);
            buffer[..size].copy_from_slice(&DATA[self.offset..self.offset + size]);
            self.offset += size;
            Ok(size)
        })
    }
}
