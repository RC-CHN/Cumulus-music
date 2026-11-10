//! Sealed media body for the real native scheduler; never projects rate or task state.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Mutex, time::Duration};

const ID: &str = "7101";
const URL: &str = "https://m1.music.126.net/cumulus-download-speed-proof.flac";
const ETAG: &str = "\"synthetic-download-speed\"";
const FLAC: &[u8] = include_bytes!("../../../core/tests/fixtures/stereo-pcm-source.flac");
const MD5: &str = "4e25192f88135eb81a995c6aa5fa7848";
const CHUNK: usize = 2_048;
const PARTIAL_LIMIT: usize = 98_304;
const CATALOG: &str = "/api/v3/song/detail";
const AUTHORIZE: &str = "/api/song/enhance/download/url/v1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    Hold,
    Flow,
    Stall,
    WakePause,
    Finish,
}

struct Trace {
    gate: Gate,
    catalog_reads: u32,
    authorizations: u32,
    rejected: u32,
    opens: Vec<Value>,
    delivered: [usize; 2],
    dropped: [bool; 2],
    pause_wake_bytes: usize,
}
static TRACE: Mutex<Trace> = Mutex::new(Trace {
    gate: Gate::Hold,
    catalog_reads: 0,
    authorizations: 0,
    rejected: 0,
    opens: Vec::new(),
    delivered: [0; 2],
    dropped: [false; 2],
    pause_wake_bytes: 0,
});

pub fn enabled() -> bool {
    std::env::var("CUMULUS_DOWNLOAD_SPEED_UI_PROOF").as_deref() == Ok("1")
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let exact_catalog = json!({"c": "[{\"id\":\"7101\",\"v\":0}]"});
    if path == CATALOG && args == &exact_catalog && !cached {
        TRACE.lock().unwrap().catalog_reads += 1;
        return Some(Ok(json!({
            "code":200,
            "songs":[{"id":7101,"name":"合成实时下载速率","dt":2000,"fee":0,
                "ar":[{"id":1,"name":"合成歌手"}],"al":{"id":2,"name":"合成专辑"}}],
            "privileges":[{"id":7101,"fee":0,"st":0,"dl":999000,"pl":320000,"maxbr":999000,"flag":0}]
        })));
    }
    if path != AUTHORIZE {
        return None;
    }
    let mut trace = TRACE.lock().unwrap();
    if cached
        || args != &json!({"id":ID,"level":"lossless","immerseType":"c51"})
        || trace.authorizations >= 2
    {
        trace.rejected += 1;
        return Some(Err(Error::State(
            "unexpected sealed download authorization",
        )));
    }
    trace.authorizations += 1;
    Some(Ok(
        json!({"code":200,"data":{"id":7101,"code":200,"url":URL,"type":"flac",
        "size":FLAC.len(),"md5":MD5,"level":"lossless"}}),
    ))
}

fn pause_requested(store: &StateStore) -> Result<bool> {
    let tasks = store.values("2", "download_tasks_v1")?;
    if let Some((id, task)) = tasks.iter().next()
        && tasks.len() == 1
        && task["song_id"] == ID
    {
        return Ok(store.get("2", "download_controls_v1", id)? == Some(json!("pause")));
    }
    Ok(false)
}

pub fn control(store: &StateStore, action: &str) -> Result<()> {
    let pause_requested = action == "wake_pause" && pause_requested(store)?;
    let mut trace = TRACE.lock().unwrap();
    trace.gate = match (trace.gate, action) {
        (_, "inspect") => return Ok(()),
        (Gate::Hold, "flow") if trace.opens.len() == 1 => Gate::Flow,
        (Gate::Flow, "stall") if trace.delivered[0] > 0 => Gate::Stall,
        (Gate::Stall, "wake_pause") if pause_requested => Gate::WakePause,
        (Gate::WakePause, "finish") if trace.opens.len() == 2 && trace.dropped[0] => Gate::Finish,
        _ => return Err(Error::State("unexpected sealed media fixture gate")),
    };
    Ok(())
}

pub fn report(store: &StateStore) -> Result<Value> {
    let pause_requested = pause_requested(store)?;
    let trace = TRACE.lock().unwrap();
    Ok(json!({
        "metadataReads":trace.catalog_reads,"authorizations":trace.authorizations,
        "rejected":trace.rejected,"opens":trace.opens,"delivered":trace.delivered,
        "dropped":trace.dropped,"pauseRequested":pause_requested,
        "pauseWakeBytes":trace.pause_wake_bytes
    }))
}

pub struct Media;
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let mut trace = TRACE.lock().unwrap();
            let index = trace.opens.len();
            let offset = usize::try_from(request.offset).ok();
            let valid = request.url.as_str() == URL
                && match index {
                    0 => offset == Some(0) && request.if_range.is_none(),
                    1 => {
                        trace.dropped[0]
                            && trace.gate == Gate::WakePause
                            && offset == trace.delivered[0].checked_sub(trace.pause_wake_bytes)
                            && offset.is_some_and(|n| n > 0 && n < FLAC.len())
                            // Ordinary-song reauthorization clears old CDN validators,
                            // but the same MD5/length preserves the durable offset.
                            && request.if_range.is_none()
                    }
                    _ => false,
                };
            if !valid {
                trace.rejected += 1;
                return Err(Error::State("unexpected sealed download media request"));
            }
            let offset = offset.unwrap();
            let mut headers = BTreeMap::from([
                ("content-length".into(), (FLAC.len() - offset).to_string()),
                ("etag".into(), ETAG.into()),
            ]);
            let status = if offset == 0 { 200 } else { 206 };
            if status == 206 {
                headers.insert(
                    "content-range".into(),
                    format!("bytes {offset}-{}/{}", FLAC.len() - 1, FLAC.len()),
                );
            }
            trace.opens.push(json!({"offset":offset,"status":status,
                "validator":request.if_range.as_deref() == Some(ETAG)}));
            Ok(MediaReply {
                status,
                headers,
                body: Box::new(Body { index, offset }),
            })
        })
    }
}

struct Body {
    index: usize,
    offset: usize,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(75)).await;
                let mut trace = TRACE.lock().unwrap();
                let end = match (trace.gate, self.index) {
                    (Gate::Flow, 0) if self.offset < PARTIAL_LIMIT => PARTIAL_LIMIT,
                    (Gate::WakePause, 0) if trace.pause_wake_bytes == 0 => FLAC.len(),
                    (Gate::Finish, 1) => FLAC.len(),
                    _ => continue,
                };
                let count = (end - self.offset).min(buffer.len()).min(CHUNK);
                buffer[..count].copy_from_slice(&FLAC[self.offset..self.offset + count]);
                self.offset += count;
                trace.delivered[self.index] += count;
                if trace.gate == Gate::WakePause {
                    // Let the in-flight read finish after the real Pause request.
                    // The transfer must reject these returned bytes at its checkpoint.
                    trace.pause_wake_bytes = count;
                }
                return Ok(count);
            }
        })
    }
}
impl Drop for Body {
    fn drop(&mut self) {
        TRACE.lock().unwrap().dropped[self.index] = true;
    }
}
