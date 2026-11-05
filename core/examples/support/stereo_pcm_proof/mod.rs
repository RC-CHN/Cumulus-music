//! Shared executable prerequisite after real GUI startup or standalone.
//! Synthetic download publication and public cold grants only; no test bypass.
mod flow;
mod runtime;
#[path = "../cold_offline_seed.rs"]
mod seed;
pub use flow::verify;

use cumulus_core::{
    Error, Result,
    protocol::{
        api::ApiFuture,
        http::{HttpBackend, HttpReply, RequestPlan},
    },
};
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Offline(AtomicUsize);
impl HttpBackend for Offline {
    fn post(&self, _: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::AcqRel);
            Err(Error::Transport(
                "sealed stereo PCM cold phase forbids all HTTP".into(),
            ))
        })
    }
}
fn ensure(value: bool, message: &'static str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::State(message))
    }
}
fn until<T>(
    label: &'static str,
    mut read: impl FnMut(&mut Value) -> Result<Option<T>>,
) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let started = Instant::now();
    let mut last = Value::Null;
    let mut next_log = Duration::ZERO;
    loop {
        match read(&mut last) {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => (),
            Err(error) => {
                eprintln!(
                    "STEREO_PCM_WAIT_FAILED {}",
                    json!({"wait":label,"elapsedMs":started.elapsed().as_millis(),"last":last})
                );
                return Err(error);
            }
        }
        if started.elapsed() >= next_log {
            eprintln!(
                "STEREO_PCM_WAIT {}",
                json!({"wait":label,"elapsedMs":started.elapsed().as_millis(),"last":last})
            );
            next_log = if next_log.is_zero() {
                Duration::from_millis(500)
            } else {
                next_log * 2
            };
        }
        if Instant::now() >= deadline {
            eprintln!(
                "STEREO_PCM_WAIT_FAILED {}",
                json!({"wait":label,"elapsedMs":started.elapsed().as_millis(),"last":last})
            );
            return Err(Error::State("paced native PCM proof timed out"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn observed(
    view: &cumulus_core::playback::engine::EngineSnapshot,
    control: &cumulus_core::playback::engine::StereoPcmControl,
) -> Result<Value> {
    Ok(
        json!({"native":{"entry":view.entry_id,"position":view.position,"duration":view.duration,"paused":view.paused,"buffering":view.buffering,"seeking":view.seeking,"audioOutput":view.audio_output,"events":format!("{:?}",view.events)},"control":state(control.snapshot()?)}),
    )
}
fn state(value: cumulus_core::playback::engine::StereoPcmSnapshot) -> Value {
    json!({
        "firstSourceFrame":value.first_source_frame,"sourceEnd":value.source_end,
        "decodedSourceFrame":value.decoded_source_frame,"producedFrames":value.produced_frames,
        "deliveredFrames":value.delivered_frames,"bufferedFrames":value.buffered_frames,
        "nativeOutputFrame":value.native_output_frame,"nativeSourceFrame":value.native_source_frame,
        "nativeEntry":value.native_entry,"paused":value.paused,"seeking":value.seeking,
        "graphEnd":value.graph_end,"requestedRevision":value.requested_revision,"appliedRevision":value.applied_revision,
        "cancelled":value.cancelled,"failed":value.failed,"readerClosed":value.reader_closed,
        "nativeSinkRetired":value.native_sink_retired,
    })
}
