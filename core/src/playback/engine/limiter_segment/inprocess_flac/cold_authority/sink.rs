//! One real native sink driven by the authorized in-process FLAC producer.
use super::{Api, FLAC_BLOCK, Fixture, Generation, RATE, replace, sdk_path, worker::Worker};
use crate::{
    Error,
    download::offline::ColdOfflineFlacAuthority,
    playback::engine::{
        AudioEngine, Cancellation, EndReason, EngineEvent, LoadedMedia, MpvConfig, MpvEngine,
        limiter_segment::{
            DecodedPacket,
            native_decoder::{SEEK_FRAME, SOURCE_END, input_pair, until},
            probe_wav::{DecodedWave, Trace, seconds},
        },
    },
};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::Ordering},
};

mod handoff;

struct Entry {
    media: LoadedMedia,
    trace: Arc<Mutex<Trace>>,
    gate: Cancellation,
}

fn stage(
    engine: &mut MpvEngine,
    worker: &Worker,
    authority: &ColdOfflineFlacAuthority,
    generation: Generation,
    first: u64,
    late: Option<DecodedPacket>,
) -> Entry {
    authority.validate().unwrap();
    let source = authority.cancellation();
    let gate = Cancellation::new(move || !source.is_cancelled());
    let trace = Arc::new(Mutex::new(Trace::default()));
    let media = engine
        .load_probe_wav(
            DecodedWave::new(
                worker.factory(gate.clone()),
                generation,
                first,
                gate.clone(),
                trace.clone(),
                late,
            ),
            gate.clone(),
        )
        .unwrap_or_else(|error| {
            panic!(
                "cold WAV stage {generation:?} at {first}: {error:?}, callback error {:?}",
                trace.lock().unwrap().failure
            )
        });
    assert_eq!(media.codec.as_deref(), Some("pcm_f32le"));
    authority.validate().unwrap();
    Entry { media, trace, gate }
}

fn wait_for_buffer(engine: &mut MpvEngine, entry: &Entry) -> f64 {
    until(|| {
        let view = engine.poll().unwrap();
        let source = entry.trace.lock().unwrap();
        let clock = view.position_for(entry.media.entry_id)?;
        (view.paused
            && source.ends > 0
            && source.pcm_high_water == SOURCE_END
            && view
                .buffered_through
                .is_some_and(|cached| cached > clock + 0.4))
        .then_some(clock)
    })
}

fn wait_for_play(engine: &mut MpvEngine, entry: &Entry) -> f64 {
    until(|| {
        let view = engine.poll().unwrap();
        let position = view.position_for(entry.media.entry_id)?;
        (!view.paused && view.audio_output.as_deref() == Some("null") && position >= 0.15)
            .then_some(position)
    })
}

#[cfg(target_os = "linux")]
fn verify_unindexed_budget(api: &Api, fixture: &Fixture) {
    use super::super::{BYTE_BUDGET, Decoder};
    let path = fixture.dir.path().join("sink-unindexed.flac");
    api.encode_fixture(&path, RATE, FLAC_BLOCK, SOURCE_END, |frame| {
        input_pair(frame).map(i32::from)
    })
    .unwrap();
    fixture.publish("sink-unindexed", &fs::read(path).unwrap(), true);
    let reader = fixture
        .grant
        .open_flac_reader("sink-unindexed", Cancellation::default())
        .unwrap();
    let authority = reader.authority().cancellation();
    let mut decoder = Decoder::open(api, Box::new(reader), Generation(1), authority).unwrap();
    for generation in [Generation(2), Generation(3)] {
        decoder.seek(generation, 0).unwrap();
        while decoder.next().unwrap().is_some() {}
    }
    let before = decoder.source().stats.bytes_read;
    let error = decoder.seek(Generation(4), SEEK_FRAME).unwrap_err();
    let read = decoder.source().stats.bytes_read - before;
    assert_eq!(error.to_string(), "FLAC operation read budget exhausted");
    assert_eq!(read, BYTE_BUDGET as u64);
    eprintln!(
        "unindexed uneven-density fixture: sample seek refused at frame {SEEK_FRAME} after {read} compressed bytes: {error}"
    );
}

#[tokio::test]
#[ignore = "requires trusted libFLAC 1.5.0 and compatible libmpv; real cold grant and null audio output"]
async fn limiter_cold_flac_sink_seek_eof_and_buffered_revocation() {
    let api = Api::open(&sdk_path()).unwrap();
    let fixture = Fixture::new().await;
    let file = fixture.dir.path().join("sink-fixture.flac");
    api.encode_fixture_with_index(
        &file,
        RATE,
        FLAC_BLOCK,
        SOURCE_END,
        Some(FLAC_BLOCK),
        |frame| input_pair(frame).map(i32::from),
    )
    .unwrap();
    fixture.membership();
    #[cfg(target_os = "linux")]
    verify_unindexed_budget(&api, &fixture);
    let path = fixture.publish("sink-protected", &fs::read(file).unwrap(), true);
    let reader = fixture
        .grant
        .open_flac_reader("sink-protected", Cancellation::default())
        .unwrap();
    let authority = reader.authority();
    replace(&path);
    assert!(
        fixture
            .grant
            .open_flac_reader("sink-protected", Cancellation::default())
            .is_err()
    );

    // Drop the native sink before the worker. The worker owns exactly one
    // native decoder and one cold File across all WAV and seek generations.
    let worker = Worker::open(reader, SOURCE_END).unwrap();
    let mut engine = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let original = stage(&mut engine, &worker, &authority, Generation(1), 0, None);
    assert!((original.media.duration.unwrap() - seconds(SOURCE_END + 1)).abs() < 0.0001);
    let paused_clock = wait_for_buffer(&mut engine, &original);
    engine.pause(false).unwrap();
    let old_clock = wait_for_play(&mut engine, &original);
    engine.pause(true).unwrap();
    let (old_pair, late) = {
        let mut trace = original.trace.lock().unwrap();
        (
            trace.seek_pair.unwrap(),
            trace.first_decoder_packet.take().unwrap(),
        )
    };
    engine.stop().unwrap();
    assert!(
        !original.gate.is_cancelled(),
        "stopping one entry must not cancel the caller's lease"
    );
    original.gate.cancel();

    let resumed = stage(
        &mut engine,
        &worker,
        &authority,
        Generation(2),
        SEEK_FRAME,
        Some(late),
    );
    assert_ne!(resumed.media.entry_id, original.media.entry_id);
    assert!((resumed.media.duration.unwrap() - seconds(SOURCE_END - SEEK_FRAME)).abs() < 0.0001);
    assert!(
        wait_for_buffer(&mut engine, &resumed) < 0.05,
        "replacement must stay paused at the segment start"
    );
    let new_pair = resumed.trace.lock().unwrap().seek_pair.unwrap();
    assert_eq!(
        new_pair,
        input_pair(SEEK_FRAME).map(|v| f32::from(v) / 32768.0)
    );
    assert!(old_pair[0] > 0.0 && old_pair[0] < new_pair[0]);
    assert_eq!(old_pair[1], -old_pair[0] * 0.125);
    engine.pause(false).unwrap();
    let mut mapped = seconds(SEEK_FRAME);
    let mut advanced = 0;
    until(|| {
        let view = engine.poll().unwrap();
        if let Some(position) = view.position_for(resumed.media.entry_id) {
            let now = seconds(SEEK_FRAME) + position;
            assert!(now + 0.02 >= mapped && now <= seconds(SOURCE_END) + 0.02);
            advanced += usize::from(now > mapped);
            mapped = mapped.max(now);
        }
        view.events.iter().any(|event| matches!(event, EngineEvent::Ended { entry_id, reason: EndReason::Eof, error: 0 } if *entry_id == resumed.media.entry_id)).then_some(())
    });
    assert!(advanced > 2 && mapped > seconds(SEEK_FRAME) + 0.25);
    let digest = {
        let trace = resumed.trace.lock().unwrap();
        assert_eq!(trace.stale_offers, 1);
        assert_eq!(trace.pcm_high_water, SOURCE_END);
        assert!(trace.ends > 0);
        hex::encode(trace.pcm_digest.unwrap())
    };
    resumed.gate.cancel();

    // Use a new native entry on the same sink and the same authorized File.
    // Show the buffer actually holds future audio while the host clock runs.
    let revoked = stage(&mut engine, &worker, &authority, Generation(3), 0, None);
    assert_ne!(revoked.media.entry_id, resumed.media.entry_id);
    wait_for_buffer(&mut engine, &revoked);
    engine.pause(false).unwrap();
    let revocation_clock = wait_for_play(&mut engine, &revoked);
    assert!(revocation_clock + 0.5 < seconds(SOURCE_END));
    let prefetched = {
        // Record the byte counter at detach, not in an earlier sampling
        // interval when a concurrent prefetch is still authorized.
        let trace = revoked.trace.lock().unwrap();
        fixture.cold.detach().unwrap();
        assert!(revoked.gate.is_cancelled());
        trace.bytes_returned
    };
    assert!(matches!(engine.poll(), Err(Error::Cancelled)));
    assert!(authority.validate().is_err());
    until(|| {
        let view = engine.poll().unwrap();
        assert_eq!(view.entry_id, None);
        assert!(view.position.is_none());
        for event in view.events {
            if let EngineEvent::Ended {
                entry_id, reason, ..
            } = event
                && entry_id == revoked.media.entry_id
            {
                assert_eq!(reason, EndReason::Stop, "revoked buffer is not natural EOF");
                return Some(());
            }
        }
        None
    });
    assert_eq!(revoked.trace.lock().unwrap().bytes_returned, prefetched);
    assert_eq!(worker.counters.opens.load(Ordering::Acquire), 1);
    let passes = worker.counters.passes.load(Ordering::Acquire);
    let seeks = worker.counters.byte_seeks.load(Ordering::Acquire);
    let max_seek_bytes = worker.counters.max_seek_bytes.load(Ordering::Acquire);
    assert!(passes >= 3 && seeks >= passes);
    assert!(max_seek_bytes > 0 && max_seek_bytes < super::super::BYTE_BUDGET as u64);
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
    eprintln!(
        "cold NCM-FLAC → limiter → libmpv: one verified descriptor/decoder, {passes} passes, {seeks} compressed byte seeks, at most {max_seek_bytes} bytes per restart; pause {paused_clock:.3}s/played {old_clock:.3}s; restart frame {SEEK_FRAME}, linked PCM {old_pair:?} → {new_pair:?}, SHA-256 {digest}, mapped host {mapped:.3}s then EOF; prefetched entry revoked at {revocation_clock:.3}s, native Stop and no more bytes"
    );
}
