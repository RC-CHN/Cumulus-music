use cumulus_core::{
    Error,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

#[derive(Default)]
pub struct ReadGate {
    pub reached: Notify,
    pub release: Notify,
}
#[derive(Clone)]
pub struct Settings {
    pub data: Vec<u8>,
    pub ignore_range: bool,
    pub corrupt_range: bool,
    pub truncate: usize,
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub range_limit: Option<usize>,
    pub status: Option<u16>,
    pub content_length: Option<String>,
    pub encoding: Option<String>,
    pub gate: Option<Arc<ReadGate>>,
    pub fail_body: bool,
}
pub struct Origin {
    pub settings: Mutex<Settings>,
    calls: Mutex<Vec<MediaRequest>>,
}
impl Origin {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            settings: Mutex::new(Settings {
                data,
                ignore_range: false,
                corrupt_range: false,
                truncate: 0,
                etag: Some("\"fixture-v1\"".into()),
                modified: None,
                range_limit: None,
                status: None,
                content_length: None,
                encoding: None,
                gate: None,
                fail_body: false,
            }),
            calls: Mutex::new(Vec::new()),
        }
    }
    pub fn calls(&self) -> Vec<MediaRequest> {
        self.calls.lock().unwrap().clone()
    }
}
impl MediaBackend for Origin {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(request.clone());
            let s = self.settings.lock().unwrap().clone();
            let start = if s.ignore_range {
                0
            } else {
                request.offset as usize
            };
            let end = (start + s.range_limit.unwrap_or(s.data.len())).min(s.data.len());
            let data = &s.data[start..end];
            let status = s.status.unwrap_or(if start > 0 || s.range_limit.is_some() {
                206
            } else {
                200
            });
            let mut headers = BTreeMap::from([(
                "content-length".into(),
                s.content_length.unwrap_or_else(|| data.len().to_string()),
            )]);
            if status == 206 {
                headers.insert(
                    "content-range".into(),
                    format!(
                        "bytes {}-{}/{}",
                        start + usize::from(s.corrupt_range),
                        end - 1,
                        s.data.len()
                    ),
                );
            }
            if let Some(v) = s.etag {
                headers.insert("etag".into(), v);
            }
            if let Some(v) = s.modified {
                headers.insert("last-modified".into(), v);
            }
            if let Some(v) = s.encoding {
                headers.insert("content-encoding".into(), v);
            }
            let body = Bytes {
                data: data[..data.len().saturating_sub(s.truncate)].to_vec(),
                offset: 0,
                gate: s.gate,
                fail: s.fail_body,
            };
            Ok(MediaReply {
                status,
                headers,
                body: Box::new(body),
            })
        })
    }
}
struct Bytes {
    data: Vec<u8>,
    offset: usize,
    gate: Option<Arc<ReadGate>>,
    fail: bool,
}
impl MediaBody for Bytes {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            if let Some(gate) = self.gate.take() {
                gate.reached.notify_one();
                gate.release.notified().await;
            }
            if self.fail {
                return Err(Error::Transport("synthetic interruption".into()));
            }
            let n = buffer.len().min(self.data.len() - self.offset);
            buffer[..n].copy_from_slice(&self.data[self.offset..self.offset + n]);
            self.offset += n;
            Ok(n)
        })
    }
}
