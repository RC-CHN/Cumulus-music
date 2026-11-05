use super::*;
use crate::{
    Error,
    effects::{
        dsp::Format,
        stereo::{LocalStereoMaterialPlan, LocalStereoSubgraph},
    },
    playback::engine::Cancellation,
};
use serde_json::json;
use std::{io::Read, sync::mpsc, thread, time::Duration};

fn plan(presence: i32) -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":presence,"stereoizer":5,"sshaper":true,"ambience":1}})).unwrap()
}

#[test]
fn paused_credit_applies_controls_but_decoder_and_reader_counts_cannot_advance_native_clock() {
    let (mut reader, control, shared) =
        channel(44100, 1000, 200000, Cancellation::default(), None).unwrap();
    control.publish_sink().unwrap();
    control.bind_entry(7).unwrap();
    let revision = control.configure(plan(-3)).unwrap();
    let (stop, done) = mpsc::sync_channel(1);
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || {
        let mut graph =
            LocalStereoSubgraph::new(Format::new(44100.0, 2, 2048).unwrap(), plan(3)).unwrap();
        let result = worker_shared.wait_for_credit(&mut graph);
        stop.send(result).unwrap();
    });
    // The applied revision is a deterministic acknowledgement that the worker
    // reached the no-credit loop, rather than a sleep hoping it was scheduled.
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    let state = control.snapshot().unwrap();
    assert_eq!(state.produced_frames, 0);
    assert_eq!(state.native_output_frame, 0);
    // An inconsistent clock ahead of delivered PCM cannot create credit.
    control.observe(7, Some(100.0), true, false).unwrap();
    assert_eq!(control.snapshot().unwrap().native_output_frame, 0);
    assert!(matches!(done.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(matches!(
        control.observe(8, Some(0.0), false, false),
        Err(Error::StaleResponse)
    ));
    control.cancel();
    assert!(matches!(
        done.recv_timeout(Duration::from_secs(2)).unwrap(),
        Err(Error::Cancelled)
    ));
    worker.join().unwrap();
    let mut bytes = [9; 8];
    assert!(reader.read(&mut bytes).is_err());
    assert_eq!(bytes, [0; 8]);
    drop(reader);
    let view = control.snapshot().unwrap();
    assert!(view.reader_closed);
    assert!(
        !view.native_sink_retired,
        "reader close alone is not an AO retirement acknowledgement"
    );
    control.retire_sink();
    assert!(control.snapshot().unwrap().native_sink_retired);
}

#[test]
fn raw_reader_keeps_partial_bytes_and_only_verified_graph_end_returns_eof() {
    let (mut reader, control, shared) =
        channel(44100, 1000, 2000, Cancellation::default(), None).unwrap();
    shared
        .publish(&[0.25, -0.5, 1.0, -1.0], 1002, false)
        .unwrap();
    let mut actual = [0u8; 16];
    for chunk in actual.chunks_mut(3) {
        reader.read_exact(chunk).unwrap();
    }
    let expected = [0.25f32, -0.5, 1.0, -1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(actual.as_slice(), expected);
    assert_eq!(control.snapshot().unwrap().native_source_frame, 1000);
    shared.publish(&[], 2000, true).unwrap();
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    drop(reader);
    let state = control.snapshot().unwrap();
    assert!(state.graph_end && state.reader_closed && !state.cancelled);
}

#[test]
fn producer_failure_zeros_even_an_already_encoded_partial_read_window() {
    let (mut reader, _control, shared) =
        channel(44100, 0, 5000, Cancellation::default(), None).unwrap();
    shared.publish(&[0.25, -0.5], 2, false).unwrap();
    let mut byte = [0];
    reader.read_exact(&mut byte).unwrap();
    shared.fail();
    let mut pending = [9; 7];
    assert!(reader.read(&mut pending).is_err());
    assert_eq!(pending, [0; 7]);
}
