use super::*;
use crate::{
    Error,
    effects::{
        dsp::Format,
        equalizer::EqualizerSettings,
        stereo::{
            LocalEqualizerMaterialPlan, LocalEqualizerStereoMaterial, LocalFixedStereoMaterial,
            LocalStereoMaterialPlan, LocalStereoSubgraph,
        },
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
        let result = worker_shared.wait_for_credit(|control| match control {
            StereoGraphControl::Cold(plan) => graph.configure(plan).map(|()| false),
            _ => panic!("unexpected ordinary graph control"),
        });
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
    // A real settled zero clock still counts as the first sync. A later
    // seek signal while paused must not reopen startup buffering.
    control.observe(7, Some(0.0), true, false).unwrap();
    control.observe(7, Some(0.0), true, true).unwrap();
    let revision = control.configure(plan(6)).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    assert_eq!(control.snapshot().unwrap().produced_frames, 0);
    assert!(matches!(done.try_recv(), Err(mpsc::TryRecvError::Empty)));
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
    assert_first_native_sync(false, 0);
}

#[test]
fn paused_replacement_can_prefill_two_native_packets_without_advancing_the_old_seek_clock() {
    assert_first_native_sync(true, 22_050);
}

#[test]
fn cold_paused_initial_sync_keeps_the_legacy_first_packet_bound() {
    use super::control::QUANTUM;
    let rate = 44100u32;
    let (mut reader, control, shared) =
        channel(rate, 0, 200000, Cancellation::default(), None).unwrap();
    control.publish_sink().unwrap();
    // Consume the real unbound startup credit in complete worker blocks.
    let samples = [0.25; QUANTUM * 2];
    for block in 1..=4 {
        shared.wait_for_credit(|_| unreachable!()).unwrap();
        shared
            .publish(&samples, (block * QUANTUM) as u64, false)
            .unwrap();
    }
    control.bind_entry(77).unwrap();
    control.observe(77, Some(0.0), true, true).unwrap();
    let mut packet = vec![0; rate as usize / 8 * 8];
    reader.read_exact(&mut packet).unwrap();
    let paused = control.snapshot().unwrap();
    assert!(paused.paused && paused.seeking);
    assert_eq!(paused.native_output_frame, 0);
    assert_eq!(paused.delivered_frames, u64::from(rate / 8));
    assert!(paused.produced_frames <= u64::from(rate.div_ceil(8)) + 4096);
    let state = shared.state.lock().unwrap();
    // There is room for more PCM after consuming one packet, but the bound
    // cold source has not been granted any additional production credit.
    assert_eq!(shared.producer_credit(&state), paused.produced_frames);
}

fn ordinary_plan(gain: f32) -> LocalEqualizerMaterialPlan {
    let mut gains = [0.0; 10];
    gains[3] = gain;
    LocalEqualizerMaterialPlan::from_saved(EqualizerSettings::new(true, gains).unwrap())
}

fn assert_first_native_sync(paused: bool, first: u64) {
    use crate::effects::dsp::PlanarBlock;
    enum Graph {
        Cold(LocalStereoSubgraph),
        Ordinary(LocalEqualizerStereoMaterial),
    }
    let rate = 44100u32;
    let (mut reader, control, shared) = if paused {
        native_channel(rate, first, 200000, Cancellation::default())
    } else {
        channel(rate, first, 200000, Cancellation::default(), None)
    }
    .unwrap();
    let queue_control = |gain| {
        if paused {
            control.queue_graph_control(StereoGraphControl::ApplyLocal(ordinary_plan(gain as f32)))
        } else {
            control.configure(plan(gain))
        }
    };
    control.publish_sink().unwrap();
    control.bind_entry(77).unwrap();
    control.observe(77, Some(0.0), paused, true).unwrap();
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || {
        let format = Format::new(rate as f32, 2, 2048).unwrap();
        let mut graph = if paused {
            Graph::Ordinary(LocalEqualizerStereoMaterial::new(format, ordinary_plan(3.0)).unwrap())
        } else {
            Graph::Cold(LocalStereoSubgraph::new(format, plan(3)).unwrap())
        };
        let mut block = PlanarBlock::new(format);
        let mut output = [0.0; 4096];
        let mut decoded = first;
        loop {
            match worker_shared.wait_for_credit(|control| match (&mut graph, control) {
                (Graph::Cold(graph), StereoGraphControl::Cold(plan)) => {
                    graph.configure(plan).map(|()| false)
                }
                (Graph::Ordinary(graph), StereoGraphControl::ApplyLocal(plan)) => {
                    graph.apply_material(plan).map(|()| false)
                }
                _ => panic!("control belongs to another stereo graph"),
            }) {
                Err(Error::Cancelled) => break,
                result => result.unwrap(),
            }
            let flow = match &mut graph {
                Graph::Cold(graph) => graph.process(&mut block, 2048, false),
                Graph::Ordinary(graph) => graph.process(&mut block, 2048, false),
            }
            .unwrap();
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
    let revision = queue_control(-3).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    let initial = control.snapshot().unwrap();
    assert_eq!(initial.native_output_frame, 0);
    assert_eq!(initial.native_source_frame, first);
    assert_eq!(initial.paused, paused);
    assert!(initial.seeking);
    assert_eq!(initial.delivered_frames, u64::from((rate / 8) * 2));
    assert!(initial.produced_frames <= u64::from(rate.div_ceil(4)) + 2048);
    assert!(!initial.graph_end);
    // A qualified zero observation must close the ordinary startup lane even
    // though no actual paused sample has advanced the native clock.
    let settled_clock = if paused { 0.0 } else { 0.05 };
    control
        .observe(77, Some(settled_clock), paused, false)
        .unwrap();
    control
        .observe(77, Some(settled_clock), paused, true)
        .unwrap();
    let revision = queue_control(7).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    let seeking = control.snapshot().unwrap();
    assert_eq!(seeking.native_output_frame == 0, paused);
    let revision = queue_control(-3).unwrap();
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

#[test]
fn ordinary_live_pcm_reserve_needs_bound_native_clock_and_freezes_on_pause_or_seek() {
    let rate = 48_000;
    let cancel = Cancellation::default();
    let (_reader, ordinary, native) = native_channel(rate, 0, 200_000, cancel.clone()).unwrap();
    let (_reader, protected, cold) = channel(rate, 0, 200_000, cancel, None).unwrap();
    let credit = |shared: &Shared| shared.producer_credit(&shared.state.lock().unwrap());
    for (control, shared) in [(&ordinary, &native), (&protected, &cold)] {
        control.bind_entry(77).unwrap();
        control.observe(77, Some(0.0), false, true).unwrap();
        assert_eq!(credit(shared), 12_000);
        {
            let mut state = shared.state.lock().unwrap();
            state.produced = 12_000;
            state.delivered = 7_200;
        }
        control.observe(77, Some(0.5), false, false).unwrap();
        assert_eq!(
            credit(shared),
            12_000,
            "credit must not outrun delivered audio"
        );
        control.observe(77, Some(0.125), false, false).unwrap();
    }
    assert_eq!(credit(&native), 6_000 + 18_000);
    assert_eq!(credit(&cold), 6_000 + 12_000);
    ordinary.observe(77, Some(0.125), true, false).unwrap();
    assert_eq!(credit(&native), 12_000);
    ordinary.observe(77, Some(0.125), false, true).unwrap();
    assert_eq!(credit(&native), 12_000);
    assert!(ordinary.observe(78, Some(0.125), false, false).is_err());
    assert_eq!(credit(&native), 12_000);
}

#[test]
fn accepted_native_speed_scales_only_the_bound_ordinary_refill() {
    let cancel = Cancellation::default();
    let (_reader, control, shared) = native_channel(48_000, 0, 200_000, cancel.clone()).unwrap();
    let capacity = shared.state.lock().unwrap().pcm.capacity();
    let credit = || shared.producer_credit(&shared.state.lock().unwrap());
    assert!(matches!(
        control.set_local_playback_speed(17, 4.0),
        Err(Error::StaleResponse)
    ));
    control.bind_entry(17).unwrap();
    assert_eq!(control.set_local_playback_speed(17, 4.0).unwrap(), 1.0);
    control.observe(17, Some(0.0), true, true).unwrap();
    assert_eq!(credit(), 54_000);
    assert_eq!(shared.state.lock().unwrap().pcm.capacity(), capacity);
    {
        let mut state = shared.state.lock().unwrap();
        state.produced = 12_000;
        state.delivered = 7_200;
    }
    control.observe(17, Some(0.125), false, false).unwrap();
    assert_eq!(credit(), 6_000 + 54_000);
    assert!(control.set_local_playback_speed(18, 1.0).is_err());
    assert!(control.set_local_playback_speed(17, f64::NAN).is_err());
    assert_eq!(control.snapshot().unwrap().native_output_frame, 6_000);
    assert_eq!(control.set_local_playback_speed(17, 1.0).unwrap(), 4.0);
    assert_eq!(credit(), 6_000 + 18_000);
    control.observe(17, Some(0.125), true, false).unwrap();
    control.set_local_playback_speed(17, 2.0).unwrap();
    assert_eq!(credit(), 12_000);
    cancel.cancel();
    assert!(matches!(
        control.set_local_playback_speed(17, 1.0),
        Err(Error::Cancelled)
    ));

    let (_reader, protected, _) =
        channel(48_000, 0, 200_000, Cancellation::default(), None).unwrap();
    protected.bind_entry(17).unwrap();
    assert!(protected.set_local_playback_speed(17, 4.0).is_err());
}

#[test]
fn native_control_ack_cannot_escape_revocation_during_worker_apply() {
    let lifetime = Cancellation::default();
    let (_reader, control, shared) = native_channel(44100, 0, 10000, lifetime.clone()).unwrap();
    control.bind_entry(42).unwrap();
    assert!(control.configure(plan(3)).is_err());
    assert_eq!(control.snapshot().unwrap().requested_revision, 0);
    let revision = control
        .queue_graph_control(StereoGraphControl::DisableLocalMaster)
        .unwrap();
    let (entered, start) = mpsc::sync_channel(1);
    let (release, proceed) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        shared.wait_for_credit(|command| {
            assert!(matches!(command, StereoGraphControl::DisableLocalMaster));
            entered.send(()).unwrap();
            proceed.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(false)
        })
    });
    start.recv_timeout(Duration::from_secs(2)).unwrap();
    lifetime.cancel();
    release.send(()).unwrap();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    assert!(matches!(
        control.wait_applied(revision, Duration::from_secs(2)),
        Err(Error::Cancelled)
    ));
    let snapshot = control.snapshot().unwrap();
    assert_eq!(snapshot.requested_revision, revision);
    assert_eq!(snapshot.applied_revision, 0);
    assert_eq!(snapshot.produced_frames, 0);
}

#[test]
fn modern_admission_closes_before_b0_drain_and_cannot_reopen_on_publication() {
    let (_reader, control, shared) =
        native_channel(48_000, 0, 8192, Cancellation::default()).unwrap();
    control.publish_sink().unwrap();
    control.bind_entry(17).unwrap();
    assert_eq!(control.modern_admission_revision(17).unwrap(), None);
    shared
        .publish_observing_graph(&[0.25, -0.25], 1, false, true)
        .unwrap();
    assert_eq!(control.modern_admission_revision(17).unwrap(), Some(0));
    shared.close_modern_admission().unwrap();
    assert!(!control.snapshot().unwrap().graph_end);
    assert_eq!(control.modern_admission_revision(17).unwrap(), None);
    shared
        .publish_observing_graph(&[-0.25, 0.25], 8192, false, true)
        .unwrap();
    assert!(
        control.validate_host().is_ok(),
        "source still owns its drain"
    );
    assert_eq!(control.modern_admission_revision(17).unwrap(), None);
}
