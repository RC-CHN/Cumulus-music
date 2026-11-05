use super::*;
use crate::{
    Error,
    effects::{
        dsp::Format,
        stereo::{LocalFixedStereoMaterial, LocalStereoMaterialPlan, LocalStereoSubgraph},
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
            LocalFixedStereoMaterial::new(Format::new(44100.0, 2, 2048).unwrap(), plan(3)).unwrap();
        let result = worker_shared.wait_for_credit(|plan| graph.configure(plan));
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
    let mut returned = 0;
    for chunk in actual.chunks_mut(3) {
        reader.read_exact(chunk).unwrap();
        returned += chunk.len();
        assert_eq!(
            control.snapshot().unwrap().delivered_frames,
            (returned / 8) as u64
        );
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
    let (mut reader, control, shared) =
        channel(44100, 0, 5000, Cancellation::default(), None).unwrap();
    shared.publish(&[0.25, -0.5], 2, false).unwrap();
    let mut byte = [0];
    reader.read_exact(&mut byte).unwrap();
    assert_eq!(control.snapshot().unwrap().delivered_frames, 0);
    shared.fail();
    let mut pending = [9; 7];
    assert!(reader.read(&mut pending).is_err());
    assert_eq!(pending, [0; 7]);
    assert_eq!(control.snapshot().unwrap().delivered_frames, 0);
}

#[test]
fn first_native_sync_can_fill_two_real_packets_without_forging_a_clock_or_loosening_later_seek() {
    use crate::effects::dsp::PlanarBlock;
    let rate = 44100u32;
    let (mut reader, control, shared) =
        channel(rate, 0, 200000, Cancellation::default(), None).unwrap();
    control.publish_sink().unwrap();
    control.bind_entry(77).unwrap();
    control.observe(77, Some(0.0), false, true).unwrap();
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || {
        let mut graph =
            LocalStereoSubgraph::new(Format::new(rate as f32, 2, 2048).unwrap(), plan(3)).unwrap();
        let mut block = PlanarBlock::new(graph.format());
        let mut output = [0.0; 4096];
        let mut decoded = 0;
        loop {
            match worker_shared.wait_for_credit(|plan| graph.configure(plan)) {
                Err(Error::Cancelled) => break,
                result => result.unwrap(),
            }
            let flow = graph.process(&mut block, 2048, false).unwrap();
            block
                .copy_interleaved(flow.valid_frames, &mut output[..flow.valid_frames * 2])
                .unwrap();
            decoded += 2048;
            match worker_shared.publish(&output[..flow.valid_frames * 2], decoded, false) {
                Err(Error::Cancelled) => break,
                result => result.unwrap(),
            }
        }
    });
    let (sent, received) = mpsc::sync_channel(1);
    let consumer = thread::spawn(move || {
        // Raw demux packets are floor(rate/8). One is not sufficient to fill
        // mpv/null's real 0.2s AO queue and finish initial syncing.
        let mut bytes = vec![0; (rate as usize / 8) * 2 * 8];
        let result = reader.read_exact(&mut bytes);
        sent.send((result, reader)).unwrap();
    });
    let read = received.recv_timeout(Duration::from_secs(2));
    if read.is_err() {
        control.cancel();
    }
    consumer.join().unwrap();
    if read.is_err() {
        worker.join().unwrap();
        panic!("initial sync never supplied two real native packets");
    }
    let (read, reader) = read.unwrap();
    read.unwrap();
    let revision = control.configure(plan(-3)).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    let initial = control.snapshot().unwrap();
    assert_eq!(initial.native_output_frame, 0);
    assert_eq!(initial.delivered_frames, u64::from((rate / 8) * 2));
    assert!(initial.produced_frames <= u64::from(rate.div_ceil(4)) + 2048);
    assert!(!initial.graph_end);
    control.observe(77, Some(0.05), false, false).unwrap();
    control.observe(77, Some(0.05), false, true).unwrap();
    let revision = control.configure(plan(7)).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    let seeking = control.snapshot().unwrap();
    assert!(seeking.native_output_frame > 0);
    let revision = control.configure(plan(-3)).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        control.snapshot().unwrap().produced_frames,
        seeking.produced_frames
    );
    control.cancel();
    worker.join().unwrap();
    drop(reader);
    assert!(!control.snapshot().unwrap().native_sink_retired);
    control.retire_sink();
}
