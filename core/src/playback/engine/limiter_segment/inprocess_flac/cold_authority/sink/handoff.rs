//! Retire real native PCM output and reclaim the original cold source.
use super::{
    Api, AudioEngine, Cancellation, EndReason, EngineEvent, Error, FLAC_BLOCK, Fixture, Generation,
    MpvConfig, MpvEngine, Ordering, PathBuf, RATE, SEEK_FRAME, SOURCE_END, Worker, fs, input_pair,
    replace, sdk_path, seconds, stage, until, wait_for_buffer, wait_for_play,
};
use crate::{
    download::offline::ColdOfflineFlacReader,
    playback::engine::limiter_segment::inprocess_flac::{
        BYTE_BUDGET, cold_authority::worker::SeekCheck,
    },
};

fn retire_exact(native: &mut MpvEngine, active: i64) -> crate::Result<()> {
    let before = native.poll()?;
    if before.entry_id != Some(active) {
        return Err(Error::State("cold FLAC seek belongs to a retired entry"));
    }
    native.stop()?;
    // For these real fixtures assert that libmpv actually acknowledged
    // this entry. The ordinary protected stop path destroys its handle
    // instead if it cannot prove Stop; see the separate pinned-stop tests.
    let after = native.poll()?;
    assert!(after.entry_id.is_none() && after.position.is_none());
    assert!(after.events.iter().any(|event| matches!(event,
        EngineEvent::Ended { entry_id, reason: EndReason::Stop, error: 0 }
        if *entry_id == active
    )));
    Ok(())
}

fn recoverable_seek(
    worker: Worker,
    target: u64,
    request: &Cancellation,
) -> crate::Result<(ColdOfflineFlacReader, u64)> {
    let SeekCheck::ReadBudget { bytes } = worker.try_seek(Generation(2), target, request)? else {
        return Err(Error::State("optional FLAC seek did not need a fallback"));
    };
    let reader = worker.reclaim()?;
    reader.authority().validate()?;
    request.check()?;
    Ok((reader, bytes))
}

#[tokio::test]
#[ignore = "requires trusted libFLAC and libmpv; active protected limiter fallback with null output"]
async fn limiter_cold_flac_active_seek_retires_sink_and_reclaims_exact_source() {
    let api = Api::open(&sdk_path()).unwrap();
    let fixture = Fixture::new().await;
    fixture.membership();
    for (name, spacing, paused, target_frame, revoked) in [
        ("active-unindexed", None, true, SEEK_FRAME, false),
        ("active-sparse", Some(8192), false, SEEK_FRAME, false),
        ("active-invalid", None, false, SOURCE_END, false),
        ("active-revoked", None, false, SEEK_FRAME, true),
    ] {
        let file = fixture.dir.path().join(format!("{name}.flac"));
        api.encode_fixture_with_index(&file, RATE, FLAC_BLOCK, SOURCE_END, spacing, |frame| {
            input_pair(frame).map(i32::from)
        })
        .unwrap();
        let path = fixture.publish(name, &fs::read(file).unwrap(), true);
        let reader = fixture
            .grant
            .open_flac_reader(name, Cancellation::default())
            .unwrap();
        let authority = reader.authority();
        replace(&path);
        assert!(
            fixture
                .grant
                .open_flac_reader(name, Cancellation::default())
                .is_err()
        );
        let worker = Worker::open(reader, SOURCE_END).unwrap();
        let mut native = MpvEngine::open(MpvConfig {
            library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
            null_output: true,
        })
        .unwrap();
        let old = stage(&mut native, &worker, &authority, Generation(1), 0, None);
        wait_for_buffer(&mut native, &old);
        native.pause(false).unwrap();
        let old_clock = wait_for_play(&mut native, &old);
        if paused {
            native.pause(true).unwrap();
        }
        let prior = native.poll().unwrap();
        assert_eq!(prior.paused, paused);
        assert!(old_clock < seconds(SEEK_FRAME));
        let old_id = old.media.entry_id;
        assert!(matches!(
            retire_exact(&mut native, old_id.checked_add(1).unwrap()),
            Err(Error::State("cold FLAC seek belongs to a retired entry"))
        ));
        assert_eq!(native.poll().unwrap().entry_id, Some(old_id));
        // Keep an independent stale sender alive to prove retirement does
        // not wait for every factory clone. It cannot create any more PCM.
        let stale_factory = worker.factory(authority.cancellation());
        retire_exact(&mut native, old_id).unwrap();
        let old_bytes = old.trace.lock().unwrap().bytes_returned;
        old.gate.cancel();

        let request = Cancellation::default();
        let handoff = recoverable_seek(worker, target_frame, &request);
        if target_frame == SOURCE_END {
            let Err(Error::Io(error)) = handoff else {
                panic!("invalid FLAC sample was misclassified as a recoverable seek");
            };
            assert!(error.to_string().contains("invalid FLAC logical seek"));
            assert!(native.poll().unwrap().entry_id.is_none());
            assert!(authority.validate().is_ok());
            eprintln!("{name}: invalid logical seek was rejected without a new sink");
            continue;
        }
        let (reader, bytes) = handoff.unwrap();
        assert_eq!(bytes, BYTE_BUDGET as u64);
        assert!(stale_factory(Generation(3), 0).is_err());
        assert!(native.poll().unwrap().entry_id.is_none());
        assert!(
            authority.validate().is_ok(),
            "worker stop is not account revoke"
        );
        if revoked {
            fixture.cold.detach().unwrap();
            assert!(matches!(
                native.resume_cold_flac_native(reader, seconds(SEEK_FRAME), prior.paused, &request),
                Err(Error::Cancelled)
            ));
            assert!(native.poll().unwrap().entry_id.is_none());
            assert_eq!(old.trace.lock().unwrap().bytes_returned, old_bytes);
            eprintln!(
                "{name}: old native Stop and worker join completed; account detach refused new output"
            );
            continue;
        }

        let target = seconds(SEEK_FRAME);
        let resumed = native
            .resume_cold_flac_native(reader, target, prior.paused, &request)
            .unwrap();
        assert_ne!(resumed.entry_id, old_id);
        assert_eq!(resumed.codec.as_deref(), Some("flac"));
        let clock = until(|| {
            let view = native.poll().unwrap();
            assert_eq!(view.paused, paused);
            assert!(view.position_for(old_id).is_none());
            view.position_for(resumed.entry_id)
        });
        assert!(clock >= target - 0.010);
        if paused {
            assert!((clock - target).abs() < 0.010);
            native.pause(false).unwrap();
        }
        let advanced = until(|| {
            let view = native.poll().unwrap();
            let clock = view.position_for(resumed.entry_id)?;
            (!view.paused && clock > target + 0.050).then_some(clock)
        });
        assert_eq!(old.trace.lock().unwrap().bytes_returned, old_bytes);
        native.stop().unwrap();
        assert!(authority.validate().is_ok());
        eprintln!(
            "{name}: old {old_id} Stop at {old_clock:.6}s, worker joined after typed {bytes}-byte seek limit; stock {} {target:.6}s → {clock:.6}s (paused={paused}) → {advanced:.6}s; old callback stayed at {old_bytes} bytes",
            resumed.entry_id
        );
    }
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
}
