//! Optional native prerequisite, not a GTK/WebView or physical-sound claim.
use super::{Api, FLAC_BLOCK, Fixture, RATE, replace, sdk_path, source_pair};
use crate::{
    Error,
    effects::stereo::LocalStereoMaterialPlan,
    playback::engine::{
        AudioEngine, Cancellation, ColdStereoSelection, ColdStereoSource, ColdStereoStart,
        EndReason, EngineEvent, MpvConfig, MpvEngine, limiter_segment::native_decoder::until,
    },
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn wait_stage<T>(stage: &str, poll: impl FnMut() -> Option<T>) -> T {
    eprintln!("stereo native stage: {stage}");
    let value = until(poll);
    eprintln!("stereo native verified: {stage}");
    value
}

fn plan(presence: i32) -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":presence,"stereoizer":5,"sshaper":true,"ambience":1}})).unwrap()
}
fn pass(source: ColdStereoSource, first: u64) -> crate::playback::engine::ColdStereoPass {
    let ColdStereoStart::Ready(pass) = source.start(first, &Cancellation::default()).unwrap()
    else {
        panic!("indexed synthetic stereo FLAC preparation unexpectedly selected stock");
    };
    *pass
}

#[tokio::test]
#[ignore = "requires trusted libFLAC 1.5.0 and compatible libmpv; actual cold NCM grant, null output; no GUI initialization"]
async fn cold_stereo_unknown_length_native_pause_controls_seek_tail_and_revocation() {
    let fixture = Fixture::new().await;
    fixture.membership();
    let api = Api::open(&sdk_path()).unwrap();
    let raw = fixture.dir.path().join("paced-stereo.flac");
    let total = u64::from(RATE) * 2 + 7;
    api.encode_fixture_with_index(&raw, RATE, FLAC_BLOCK, total, Some(FLAC_BLOCK), source_pair)
        .unwrap();
    let path = fixture.publish("stereo", &fs::read(raw).unwrap(), true);
    let reader = fixture
        .grant
        .open_flac_reader("stereo", Cancellation::default())
        .unwrap();
    let ColdStereoSelection::Stock(reader) =
        ColdStereoSource::select(reader, &fixture.dir.path().join("missing-sdk"), plan(3)).unwrap()
    else {
        panic!("absent trusted runtime must preserve stock held reader");
    };
    let ColdStereoSelection::Qualified(source) =
        ColdStereoSource::select(reader, &sdk_path(), plan(3)).unwrap()
    else {
        panic!("verified stereo fixture was not qualified");
    };
    assert_eq!(source.total_frames(), total);
    replace(&path);
    let mut initial = pass(*source, 0);
    let control = initial.control();
    let mut native = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let old = native
        .load_stereo_pcm(initial.take_reader().unwrap(), &Cancellation::default())
        .unwrap();
    assert_eq!(old.codec.as_deref(), Some("pcm_f32le"));
    assert!(
        old.duration.is_none(),
        "unknown PCM length must not masquerade as a finite WAV"
    );
    let revision = control.configure(plan(-3)).unwrap();
    control
        .wait_applied(revision, Duration::from_secs(2))
        .unwrap();
    let paused = native.poll().unwrap();
    assert!(paused.paused);
    assert_eq!(paused.entry_id, Some(old.entry_id));
    let prepared = control.snapshot().unwrap();
    assert!(prepared.decoded_source_frame < total && !prepared.graph_end);
    assert!(prepared.produced_frames <= u64::from(RATE.div_ceil(8)) + 4096);
    assert_eq!(prepared.native_output_frame, 0);
    eprintln!("stereo native prepared: {paused:?}; {prepared:?}");
    native.pause(false).unwrap();
    let mut traced = Instant::now();
    let before_seek = wait_stage("first native clock", || {
        let view = native.poll().unwrap();
        assert_eq!(view.entry_id, Some(old.entry_id));
        let state = control.snapshot().unwrap();
        if traced.elapsed() >= Duration::from_millis(500) {
            eprintln!("stereo first clock: {view:?}; {state:?}");
            traced = Instant::now();
        }
        assert!(
            state.produced_frames <= state.native_output_frame + u64::from(RATE.div_ceil(4)) + 2048
        );
        (view.audio_output.as_deref() == Some("null")
            && view.position_for(old.entry_id).is_some_and(|n| n >= 0.15))
        .then_some(state.native_source_frame)
    });
    let live_revision = control.configure(plan(7)).unwrap();
    control
        .wait_applied(live_revision, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        native.poll().unwrap().entry_id,
        Some(old.entry_id),
        "controls do not reload the song"
    );
    native.pause(true).unwrap();
    assert!(
        native.seek(0.5).is_err(),
        "raw bytes must not bypass the logical source handoff"
    );
    native.stop().unwrap();
    wait_stage("stopped reader closed", || {
        control.snapshot().unwrap().reader_closed.then_some(())
    });
    assert!(control.snapshot().unwrap().native_sink_retired);
    let source = initial.reclaim().unwrap();
    let first = u64::from(RATE) / 2;
    let mut resumed = pass(source, first);
    let resumed_control = resumed.control();
    let new = native
        .load_stereo_pcm(resumed.take_reader().unwrap(), &Cancellation::default())
        .unwrap();
    assert_ne!(new.entry_id, old.entry_id);
    assert!(
        native.poll().unwrap().paused,
        "logical seek preserves the caller's paused state"
    );
    native.pause(false).unwrap();
    traced = Instant::now();
    wait_stage("seeked native EOF", || {
        let view = native.poll().unwrap();
        if traced.elapsed() >= Duration::from_millis(500) {
            eprintln!(
                "stereo seeked EOF: {view:?}; {:?}",
                resumed_control.snapshot().unwrap()
            );
            traced = Instant::now();
        }
        view.events.iter().any(|event|matches!(event,EngineEvent::Ended{entry_id,reason:EndReason::Eof,error:0} if *entry_id==new.entry_id)).then_some(())
    });
    let complete = resumed_control.snapshot().unwrap();
    assert!(complete.graph_end && complete.native_sink_retired);
    assert_eq!(complete.decoded_source_frame, total);
    assert_eq!(complete.produced_frames, total - first + u64::from(RATE));
    assert_eq!(complete.native_source_frame, total);
    wait_stage("EOF reader closed", || {
        resumed_control
            .snapshot()
            .unwrap()
            .reader_closed
            .then_some(())
    });
    native.stop().unwrap();
    let source = resumed.reclaim().unwrap();
    let mut revoked = pass(source, 0);
    let revoke_control = revoked.control();
    let last = native
        .load_stereo_pcm(revoked.take_reader().unwrap(), &Cancellation::default())
        .unwrap();
    native.pause(false).unwrap();
    wait_stage("revocation native clock", || {
        native
            .poll()
            .unwrap()
            .position_for(last.entry_id)
            .is_some_and(|n| n > 0.05)
            .then_some(())
    });
    assert!(revoke_control.snapshot().unwrap().buffered_frames > 0);
    fixture.cold.detach().unwrap();
    assert!(matches!(native.poll(), Err(Error::Cancelled)));
    wait_stage("revoked reader closed", || {
        revoke_control
            .snapshot()
            .unwrap()
            .reader_closed
            .then_some(())
    });
    assert!(revoke_control.snapshot().unwrap().native_sink_retired);
    assert!(
        revoked.reclaim().is_err(),
        "revoked authority cannot recover a publishable source"
    );
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
    eprintln!(
        "cold NCM-FLAC same held descriptor -> paced SE1/default-B0 -> unknown-size raw native sink: actual first clock {before_seek}, in-place control revisions {revision}/{live_revision}, stop/close/join before seek {first}, {} real EOF frames including tail; buffered revoke stopped native entry; no GUI/physical-output claim",
        complete.produced_frames
    );
}
