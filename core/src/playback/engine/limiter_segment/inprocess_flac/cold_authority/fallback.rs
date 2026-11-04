//! Actual failed optional decode, followed by a stock native same-file handoff.
use super::{Api, Decoder, FLAC_BLOCK, Fixture, Generation, RATE, replace, sdk_path};
use crate::{
    Error,
    download::offline::ColdOfflineFlacReader,
    playback::engine::{
        AudioEngine, Cancellation, MpvConfig, MpvEngine,
        limiter_segment::{
            native_decoder::{SEEK_FRAME, SOURCE_END, input_pair, until},
            probe_wav::seconds,
        },
    },
};
use std::{fs, path::PathBuf, sync::atomic::Ordering};

fn engine() -> MpvEngine {
    MpvEngine::open(MpvConfig {
        library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
        null_output: true,
    })
    .unwrap()
}

fn preflight(api: &Api, reader: &mut ColdOfflineFlacReader) -> u64 {
    let authority = reader.authority();
    let mut probe = Decoder::open(
        api,
        Box::new(reader),
        Generation(1),
        authority.cancellation(),
    )
    .unwrap();
    // Ask for the target while mpv is idle, with no previous warm decoder
    // pass or externally visible playback generation.
    let before = probe.source().stats.bytes_read;
    let failure = probe.seek(Generation(2), SEEK_FRAME).unwrap_err();
    let bytes = probe.source().stats.bytes_read - before;
    assert_eq!(failure.to_string(), "FLAC operation read budget exhausted");
    assert_eq!(bytes, super::super::BYTE_BUDGET as u64);
    drop(probe); // finish/delete libFLAC before handing back the same reader
    authority.validate().unwrap();
    bytes
}

#[tokio::test]
#[ignore = "requires trusted libFLAC 1.5.0 and compatible libmpv; protected stock fallback with null output"]
async fn limiter_cold_flac_fallback_preserves_the_held_file_and_native_transport() {
    let api = Api::open(&sdk_path()).unwrap();
    let fixture = Fixture::new().await;
    fixture.membership();
    let mut native = engine();
    let target = seconds(SEEK_FRAME);
    for (id, index_spacing, paused) in [
        ("fallback-no-index", None, true),
        ("fallback-sparse", Some(8192), false),
    ] {
        let file = fixture.dir.path().join(format!("{id}.flac"));
        api.encode_fixture_with_index(
            &file,
            RATE,
            FLAC_BLOCK,
            SOURCE_END,
            index_spacing,
            |frame| input_pair(frame).map(i32::from),
        )
        .unwrap();
        let path = fixture.publish(id, &fs::read(file).unwrap(), true);
        let mut reader = fixture
            .grant
            .open_flac_reader(id, Cancellation::default())
            .unwrap();
        let authority = reader.authority();
        replace(&path);
        assert!(
            fixture
                .grant
                .open_flac_reader(id, Cancellation::default())
                .is_err()
        );
        let bytes = preflight(&api, &mut reader);
        assert!(native.poll().unwrap().entry_id.is_none());

        let media = native
            .resume_cold_flac_native(reader, target, paused, &Cancellation::default())
            .unwrap();
        assert_eq!(media.codec.as_deref(), Some("flac"));
        assert!((media.duration.unwrap() - seconds(SOURCE_END)).abs() < 0.01);
        let first = until(|| {
            let snapshot = native.poll().unwrap();
            let position = snapshot.position_for(media.entry_id)?;
            assert_eq!(snapshot.paused, paused);
            Some(position)
        });
        assert!(
            first >= target - 0.010,
            "fallback escaped before the target: {first}"
        );
        if paused {
            assert!((first - target).abs() <= 0.010);
            let still = native.poll().unwrap();
            assert!(
                still.paused
                    && still
                        .position_for(media.entry_id)
                        .is_some_and(|position| (position - first).abs() < 0.010)
            );
            native.pause(false).unwrap();
        }
        let advanced = until(|| {
            let snapshot = native.poll().unwrap();
            let position = snapshot.position_for(media.entry_id)?;
            (!snapshot.paused && position > target + 0.075).then_some(position)
        });
        native.stop().unwrap();
        assert!(authority.validate().is_ok());
        assert!(native.poll().unwrap().entry_id.is_none());
        eprintln!(
            "{id}: optional seek refused after {bytes} bytes with no live sink; replaced path rejected, same held NCM-FLAC at {target:.6}s → {first:.6}s (paused={paused}) → {advanced:.6}s"
        );
    }
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
}

#[tokio::test]
#[ignore = "requires compatible libmpv; failure injection on a protected stock fallback"]
async fn limiter_cold_flac_fallback_does_not_publish_bad_position_or_revoked_source() {
    let fixture = Fixture::new().await;
    let seed = super::FLAC;
    fixture.membership();
    fixture.publish("fallback-old", seed, true);
    fixture.publish("fallback-candidate", seed, true);
    let mut native = engine();
    let cancel = Cancellation::default();
    let existing = fixture
        .grant
        .open_flac_reader("fallback-old", Cancellation::default())
        .unwrap();
    let live = native
        .resume_cold_flac_native(existing, 0.0, true, &cancel)
        .unwrap();
    let candidate = fixture
        .grant
        .open_flac_reader("fallback-candidate", Cancellation::default())
        .unwrap();
    assert!(matches!(
        native.resume_cold_flac_native(candidate, 0.1, false, &cancel),
        Err(Error::State("cold FLAC fallback requires an idle sink"))
    ));
    let still = native.poll().unwrap();
    assert_eq!(still.entry_id, Some(live.entry_id));
    assert!(still.paused);
    native.stop().unwrap();

    let candidate = fixture
        .grant
        .open_flac_reader("fallback-candidate", Cancellation::default())
        .unwrap();
    assert!(matches!(
        native.resume_cold_flac_native(candidate, 60.0, false, &cancel),
        Err(Error::InvalidInput(
            "cold FLAC position is outside the native track"
        ))
    ));
    let idle = native.poll().unwrap();
    assert_eq!(idle.entry_id, None);
    assert!(idle.position.is_none());

    let candidate = fixture
        .grant
        .open_flac_reader("fallback-candidate", Cancellation::default())
        .unwrap();
    fixture.cold.detach().unwrap();
    assert!(matches!(
        native.resume_cold_flac_native(candidate, 0.1, false, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(native.poll().unwrap().entry_id.is_none());
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
    eprintln!(
        "stock fallback kept the other active entry untouched, retired an out-of-range candidate, and did not publish the revoked source"
    );
}
