//! Test the actual CLI decoder, limiter and libmpv transport in one session.
use super::{NativeProducer, SEEK_FRAME, SOURCE_END, input_pair, make_flac, until};
use crate::playback::engine::{
    AudioEngine, Cancellation, EndReason, EngineEvent, MpvConfig, MpvEngine,
    limiter_segment::{
        Generation,
        probe_wav::{DecodedWave, FRAME_BYTES, HEADER, StartPass, Trace, seconds},
    },
};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn cli(ffmpeg: OsString, flac: PathBuf) -> StartPass {
    Box::new(move |generation, first| {
        let producer = NativeProducer::start(&ffmpeg, &flac, generation, first);
        Ok(Box::new(move || producer.receive()))
    })
}

#[test]
#[ignore = "requires compatible libmpv and trusted FFmpeg; runs native FLAC-to-libmpv with null output"]
fn limiter_native_pipeline_generation_seek_and_eof() {
    let ffmpeg =
        std::env::var_os("CUMULUS_LIMITER_PROBE_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
    let dir = tempfile::tempdir().unwrap();
    let flac = make_flac(&ffmpeg, dir.path());
    let mut engine = MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap();
    let old = Generation(1);
    let old_auth = Cancellation::default();
    let old_trace = Arc::new(Mutex::new(Trace::default()));
    let original = engine
        .load_probe_wav(
            DecodedWave::new(
                cli(ffmpeg.clone(), flac.clone()),
                old,
                0,
                old_auth.clone(),
                old_trace.clone(),
                None,
            ),
            old_auth.clone(),
        )
        .unwrap();
    assert_eq!(original.codec.as_deref(), Some("pcm_f32le"));
    assert!((original.duration.unwrap() - seconds(SOURCE_END + 1)).abs() < 0.0001);
    let (paused_host, prepared) = until(|| {
        let snapshot = engine.poll().unwrap();
        let trace = old_trace.lock().unwrap();
        let position = snapshot.position_for(original.entry_id)?;
        (snapshot.paused && trace.seek_pair.is_some() && trace.ends > 0)
            .then_some((position, trace.pcm_high_water))
    });
    assert!(
        seconds(prepared) > paused_host + 0.5,
        "decoded cursor is not the playback clock"
    );
    engine.pause(false).unwrap();
    let before_seek = until(|| {
        let snapshot = engine.poll().unwrap();
        snapshot
            .position_for(original.entry_id)
            .filter(|pos| *pos >= 0.15)
    });
    engine.pause(true).unwrap();
    let old_pair;
    let late;
    {
        let mut trace = old_trace.lock().unwrap();
        old_pair = trace.seek_pair.unwrap();
        late = trace.first_decoder_packet.take().unwrap();
        assert_eq!(late.generation, old);
        eprintln!(
            "old entry {} paused at {:.3}s, decode high-water {} frames; play advanced to {:.3}s; {} byte callbacks, {} decoder passes",
            original.entry_id,
            paused_host,
            prepared,
            before_seek,
            trace.seeks.len(),
            trace.passes
        );
    }
    engine.stop().unwrap();
    old_auth.cancel();
    let generation = Generation(2);
    let new_auth = Cancellation::default();
    let trace = Arc::new(Mutex::new(Trace::default()));
    let resumed = engine
        .load_probe_wav(
            DecodedWave::new(
                cli(ffmpeg, flac),
                generation,
                SEEK_FRAME,
                new_auth.clone(),
                trace.clone(),
                Some(late),
            ),
            new_auth,
        )
        .unwrap();
    assert_ne!(resumed.entry_id, original.entry_id);
    assert_eq!(resumed.codec.as_deref(), Some("pcm_f32le"));
    let remaining = seconds(SOURCE_END - SEEK_FRAME);
    assert!((resumed.duration.unwrap() - remaining).abs() < 0.0001);
    let new_pair = until(|| {
        let snapshot = engine.poll().unwrap();
        assert!(snapshot.paused, "logical seek lost the pause state");
        trace.lock().unwrap().seek_pair
    });
    let expected = input_pair(SEEK_FRAME).map(|v| f32::from(v) / 32768.0);
    assert_eq!(new_pair, expected);
    assert!(old_pair[0] > 0.0 && old_pair[0] < new_pair[0]);
    assert_eq!(old_pair[1], -old_pair[0] * 0.125);

    engine.pause(false).unwrap();
    let mut mapped_clock = seconds(SEEK_FRAME);
    let mut ticks = 0;
    let mut null = false;
    until(|| {
        let snapshot = engine.poll().unwrap();
        null |= snapshot.audio_output.as_deref() == Some("null");
        if let Some(pos) = snapshot.position_for(resumed.entry_id) {
            let mapped = seconds(SEEK_FRAME) + pos;
            assert!(mapped + 0.02 >= mapped_clock && mapped <= seconds(SOURCE_END) + 0.02);
            ticks += usize::from(mapped > mapped_clock);
            mapped_clock = mapped_clock.max(mapped);
        }
        snapshot.events.iter().any(|event| matches!(event, EngineEvent::Ended { entry_id, reason: EndReason::Eof, error: 0 } if *entry_id == resumed.entry_id)).then_some(())
    });
    let trace = trace.lock().unwrap();
    assert!(null && ticks > 2 && mapped_clock >= seconds(SEEK_FRAME) + 0.25);
    assert_eq!(trace.stale_offers, 1);
    assert_eq!(trace.pcm_high_water, SOURCE_END);
    assert!(trace.ends > 0);
    let digest = hex::encode(trace.pcm_digest.unwrap());
    assert!(trace.bytes_returned >= HEADER + (SOURCE_END - SEEK_FRAME) as usize * FRAME_BYTES);
    eprintln!(
        "new entry {} at frame {SEEK_FRAME}: old/new pairs {old_pair:?} -> {new_pair:?}; old PCM rejected, decode reached {} frames ({} terminal passes, {} callback bytes, PCM SHA-256 {digest}); mapped libmpv clock {:.3}s across {ticks} ticks then native EOF; libmpv API {:#x}",
        resumed.entry_id,
        trace.pcm_high_water,
        trace.ends,
        trace.bytes_returned,
        mapped_clock,
        engine.api_version()
    );
}
