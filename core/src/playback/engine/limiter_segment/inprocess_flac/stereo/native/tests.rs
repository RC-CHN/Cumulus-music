use super::*;
use crate::{
    library::{identity::fingerprint, native_open::checked_audio_file},
    playback::engine::pcm_wave::tests::{wave, wave_at},
};
use serde_json::json;
use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

const FLAC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stereo-pcm-source.flac"
));

fn plan(presence: i32) -> LocalEqualizerMaterialPlan {
    LocalEqualizerMaterialPlan::from_equalizer_json(&json!({
        "eq":{"on":true,"eqs":[-1.5,-1,0,1.79,4,3.79,1.79,0.5,-1,1]},
        "bt":{"on":true,"bass":4,"treble":-3},
        "se":{"on":true,"presence":presence,"stereoizer":5,"sshaper":true,"ambience":1},
        "rvb":{"on":false},
    }))
    .unwrap()
}

fn checked_file(root: &Path, name: &str, bytes: &[u8]) -> (File, std::path::PathBuf) {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    let path = path.canonicalize().unwrap();
    let (file, format) = checked_audio_file(&path, &fingerprint(&path).unwrap()).unwrap();
    assert_eq!(format, "flac");
    (file, path)
}

#[test]
fn native_flac_stock_fallback_keeps_the_checked_handle_and_lifetime() {
    let dir = tempfile::tempdir().unwrap();
    let (file, path) = checked_file(dir.path(), "owned.flac", FLAC);
    fs::rename(&path, dir.path().join("moved.flac")).unwrap();
    fs::write(&path, b"replacement path must never be opened").unwrap();
    let lifetime = Cancellation::default();
    let NativeStereoSelection::Stock(file) = NativeStereoSource::select(
        file,
        lifetime.clone(),
        &dir.path().join("missing-flac-runtime"),
        plan(3),
    )
    .unwrap() else {
        panic!("missing decoder must keep the verified file for stock")
    };
    let (mut file, authority) = file.into_stock().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, FLAC);
    lifetime.cancel();
    assert!(matches!(authority.check(), Err(Error::Cancelled)));
}

#[test]
fn native_flac_cannot_recover_the_file_after_observed_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let (file, _) = checked_file(dir.path(), "revoked.flac", FLAC);
    let enabled = Arc::new(AtomicBool::new(true));
    let valid = enabled.clone();
    let lifetime = Cancellation::new(move || valid.load(Ordering::Acquire));
    let NativeStereoSelection::Stock(mut file) = NativeStereoSource::select(
        file,
        lifetime,
        &dir.path().join("missing-flac-runtime"),
        plan(3),
    )
    .unwrap() else {
        panic!("missing decoder must preserve the valid native file")
    };
    enabled.store(false, Ordering::Release);
    let mut buffer = [0xAA; 16];
    assert_eq!(
        file.read(&mut buffer).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(buffer, [0xAA; 16]);
    enabled.store(true, Ordering::Release);
    assert!(matches!(file.into_stock(), Err(Error::Cancelled)));
}

// The frozen FLAC's independent generator formula. With all wrappers off
// after Reset and samples far below B0's threshold, native output at the new
// source position must equal these original samples bit-for-bit.
fn dry_prefix(first: u64, frames: u64) -> Vec<u8> {
    (first..first + frames)
        .flat_map(frozen_samples)
        .flat_map(|sample| (f32::from(sample) / 32768.0).to_le_bytes())
        .collect()
}

fn frozen_samples(frame: u64) -> [i16; 2] {
    let left = ((frame * 97 + (frame / 128) * 23) % 8192) as i16 - 4096;
    [left, -(left / 8)]
}

fn wave_source(path: &Path, bytes: &[u8], authority: Cancellation) -> Box<NativeStereoSource> {
    fs::write(path, bytes).unwrap();
    let path = path.canonicalize().unwrap();
    let (file, format) = checked_audio_file(&path, &fingerprint(&path).unwrap()).unwrap();
    assert_eq!(format, "wav");
    let NativeStereoSelection::Qualified(source) =
        NativeStereoSource::select_wave(file, authority, plan(3)).unwrap()
    else {
        panic!("signed stereo PCM does not need any decoder runtime")
    };
    source
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn indexed_wave_unsupported_layout_returns_same_handle_to_stock() {
    // Keep a valid RIFF stream, but describe its original 8 bytes as four
    // mono frames. The DSP path permits only signed-16 stereo.
    let mut mono = wave([[1, -1], [2, -2]]);
    mono[22..24].copy_from_slice(&1_u16.to_le_bytes());
    mono[28..32].copy_from_slice(&(44_100_u32 * 2).to_le_bytes());
    mono[32..34].copy_from_slice(&2_u16.to_le_bytes());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("indexed.wav");
    fs::write(&path, &mono).unwrap();
    let file = File::open(&path).unwrap();
    fs::rename(&path, dir.path().join("original.wav")).unwrap();
    fs::write(&path, wave([[0, 0]])).unwrap();

    let authority = Cancellation::default();
    let NativeStereoSelection::Stock(file) =
        NativeStereoSource::select_indexed_wave(file, authority.clone(), plan(3)).unwrap()
    else {
        panic!("valid mono WAV must remain playable by the stock sink");
    };
    let (mut file, fallback_authority) = file.into_stock().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, mono);
    authority.cancel();
    assert!(matches!(fallback_authority.check(), Err(Error::Cancelled)));
}

#[test]
fn native_pcm_wav_retains_the_actual_file_and_frame_seek_after_rename() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("original.wav");
    let bytes = wave((0..2055).map(frozen_samples));
    let lifetime = Cancellation::default();
    let mut source = wave_source(&path, &bytes, lifetime.clone());
    fs::rename(&path, dir.path().join("moved.wav")).unwrap();
    fs::write(&path, b"another file cannot replace a held descriptor").unwrap();
    source.configure_reclaimed(plan(3), true).unwrap();

    let cancel = Cancellation::default();
    let NativeStereoStart::Ready(mut pass) = source.start(2048, &cancel).unwrap() else {
        panic!("seven remaining source frames must prepare")
    };
    cancel.cancel();
    let mut pcm = pass.take_reader().unwrap();
    let control = pass.control();
    assert_eq!(control.snapshot().unwrap().first_source_frame, 2048);
    control.publish_sink().unwrap();
    control.bind_entry(44).unwrap();
    control.observe(44, Some(0.0), false, true).unwrap();
    let mut prefix = [0; 56];
    pcm.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix.as_slice(), dry_prefix(2048, 7).as_slice());
    drop(pcm);
    control.retire_sink();
    let source = pass.reclaim().unwrap();
    assert_eq!(source.total_frames(), 2055);
    let (mut file, authority) = source.into_reader().unwrap().into_stock().unwrap();
    let mut returned = Vec::new();
    file.read_to_end(&mut returned).unwrap();
    assert_eq!(returned, bytes);
    lifetime.cancel();
    assert!(matches!(authority.check(), Err(Error::Cancelled)));
}

#[test]
fn native_pcm_wav_cannot_recover_to_stock_if_held_layout_changed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("editable.wav");
    let source = wave_source(&path, &wave([[0, 0]; 10]), Cancellation::default());
    fs::write(&path, wave([[1, -1]; 11])).unwrap();
    let NativeStereoStart::Stock(source) = source.start(0, &Cancellation::default()).unwrap()
    else {
        panic!("the old layout must not publish a native sink")
    };
    assert!(matches!(
        source.into_reader().unwrap().into_stock(),
        Err(Error::Media("held PCM WAV layout changed"))
    ));
}

fn drain_active_pcm(source: NativeStereoSource, first: u64, rate: u32) -> Vec<u8> {
    let total = source.total_frames();
    let NativeStereoStart::Ready(mut pass) = source.start(first, &Cancellation::default()).unwrap()
    else {
        panic!("trusted source must prepare")
    };
    let mut reader = pass.take_reader().unwrap();
    let control = pass.control();
    assert_eq!(reader.rate(), rate);
    assert_eq!(control.rate(), rate);
    control.publish_sink().unwrap();
    control.bind_entry(18).unwrap();
    control.observe(18, Some(0.0), false, true).unwrap();
    let mut pcm = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let n = reader.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        pcm.extend_from_slice(&chunk[..n]);
        control
            .observe(
                18,
                Some(pcm.len() as f64 / 8.0 / f64::from(rate)),
                false,
                false,
            )
            .unwrap();
    }
    let state = control.snapshot().unwrap();
    assert_eq!(state.decoded_source_frame, total);
    assert_eq!(state.native_source_frame, total);
    assert!(state.graph_end);
    assert_eq!(pcm.len() as u64 / 8, total - first + u64::from(rate));
    pcm
}

#[test]
#[ignore = "compares full real WAV and libFLAC 1.5.0 PCM with the same source; no audio device"]
fn active_wav_and_trusted_flac_match_every_graph_sample_and_tail() {
    let library = super::super::super::sdk_path();
    let dir = tempfile::tempdir().unwrap();
    let bytes = wave((0..88207).map(frozen_samples));
    for first in [0, 22050] {
        let wav = wave_source(
            &dir.path().join("equivalent.wav"),
            &bytes,
            Cancellation::default(),
        );
        let (file, _) = checked_file(dir.path(), "equivalent.flac", FLAC);
        let NativeStereoSelection::Qualified(flac) =
            NativeStereoSource::select(file, Cancellation::default(), &library, plan(3)).unwrap()
        else {
            panic!("a qualified FLAC reference is required")
        };
        assert_eq!(
            drain_active_pcm(*wav, first, 44_100),
            drain_active_pcm(*flac, first, 44_100)
        );
    }
}

#[test]
#[ignore = "real trusted libFLAC 1.5.0 encoder/decoder and paced output, no audio device"]
fn native_48k_flac_and_wave_share_source_rate_but_not_cold_flac_authority() {
    use super::super::super::{Api, Decoder, Generation};
    let library = super::super::super::sdk_path();
    let api = Api::open(&library).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .canonicalize()
        .unwrap()
        .join("equivalent-48000.flac");
    let total = 96_007;
    api.encode_fixture_with_index(&path, 48_000, 1024, total, Some(1024), |frame| {
        frozen_samples(frame).map(i32::from)
    })
    .unwrap();
    let fingerprint = fingerprint(&path).unwrap();
    let (file, _) = checked_audio_file(&path, &fingerprint).unwrap();
    let reader = NativeStereoFile::new(file, Cancellation::default()).unwrap();
    assert!(matches!(
        HeldSource::select_flac(reader, &library, FlacProfile::Cold, |_| panic!(
            "cold rejects 48 kHz"
        ))
        .unwrap(),
        Selection::Stock(_)
    ));
    let (file, _) = checked_audio_file(&path, &fingerprint).unwrap();
    let mut old_decoder =
        Decoder::open(&api, Box::new(file), Generation(1), Cancellation::default()).unwrap();
    assert!(
        old_decoder.next().is_err(),
        "cold decoder callback must still reject 48 kHz"
    );

    let bytes = wave_at(48_000, (0..total).map(frozen_samples));
    for first in [0, 24_000] {
        let wav = wave_source(
            &dir.path().join("equivalent-48000.wav"),
            &bytes,
            Cancellation::default(),
        );
        let (file, format) = checked_audio_file(&path, &fingerprint).unwrap();
        assert_eq!(format, "flac");
        let NativeStereoSelection::Qualified(flac) =
            NativeStereoSource::select(file, Cancellation::default(), &library, plan(3)).unwrap()
        else {
            panic!("48 kHz is allowed on native files only")
        };
        assert_eq!(
            drain_active_pcm(*wav, first, 48_000),
            drain_active_pcm(*flac, first, 48_000)
        );
    }
}

#[test]
#[ignore = "real trusted libFLAC 1.5.0 and paced fixed graph, no audio device"]
fn indexed_4608_flac_matches_1024_flac_and_wav_through_seek_and_tail() {
    use super::super::super::{Api, FLAC_BLOCK, INDEXED_FLAC_BLOCK};
    let library = super::super::super::sdk_path();
    let api = Api::open(&library).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let total = u64::from(INDEXED_FLAC_BLOCK) * 3 + 7;

    for rate in [44_100, 48_000] {
        let wide = dir.path().join(format!("wide-{rate}.flac"));
        let narrow = dir.path().join(format!("narrow-{rate}.flac"));
        for (path, block) in [(&wide, INDEXED_FLAC_BLOCK), (&narrow, FLAC_BLOCK)] {
            api.encode_fixture_with_index(path, rate, block, total, Some(block), |frame| {
                frozen_samples(frame).map(i32::from)
            })
            .unwrap();
        }
        let open = |path: &Path| {
            let path = path.canonicalize().unwrap();
            checked_audio_file(&path, &fingerprint(&path).unwrap())
                .unwrap()
                .0
        };
        let original = fs::read(&wide).unwrap();
        assert_eq!(&original[10..12], &INDEXED_FLAC_BLOCK.to_be_bytes()[2..]);
        let NativeStereoSelection::Stock(reader) =
            NativeStereoSource::select(open(&wide), Cancellation::default(), &library, plan(3))
                .unwrap()
        else {
            panic!("native Open File must retain its original callback limit")
        };
        let (mut file, _) = reader.into_stock().unwrap();
        let mut stock = Vec::new();
        file.read_to_end(&mut stock).unwrap();
        assert_eq!(stock, original);
        let reader = NativeStereoFile::new(open(&wide), Cancellation::default()).unwrap();
        assert!(matches!(
            HeldSource::select_flac(reader, &library, FlacProfile::Cold, |_| panic!(
                "cold must reject larger callback blocks"
            ))
            .unwrap(),
            Selection::Stock(_)
        ));

        let wave = wave_at(rate, (0..total).map(frozen_samples));
        for first in [0, u64::from(INDEXED_FLAC_BLOCK) + 513] {
            let wav = wave_source(
                &dir.path().join("reference.wav"),
                &wave,
                Cancellation::default(),
            );
            let NativeStereoSelection::Qualified(wide) = NativeStereoSource::select_indexed(
                open(&wide),
                Cancellation::default(),
                &library,
                plan(3),
            )
            .unwrap() else {
                panic!("eligible indexed 4608-frame FLAC must qualify")
            };
            let NativeStereoSelection::Qualified(narrow) = NativeStereoSource::select(
                open(&narrow),
                Cancellation::default(),
                &library,
                plan(3),
            )
            .unwrap() else {
                panic!("existing 1024-frame FLAC must still qualify")
            };
            let expected = drain_active_pcm(*wav, first, rate);
            assert_eq!(drain_active_pcm(*narrow, first, rate), expected);
            assert_eq!(drain_active_pcm(*wide, first, rate), expected);
        }
    }
}

#[test]
#[ignore = "requires the trusted native libFLAC 1.5.0; synthetic sink acknowledgements, no audio device"]
fn native_flac_owned_worker_controls_reclaim_seek_and_revoke() {
    let library = super::super::super::sdk_path();
    let dir = tempfile::tempdir().unwrap();
    let (file, _) = checked_file(dir.path(), "active.flac", FLAC);
    let deadline = Instant::now() + Duration::from_secs(8);
    let lifetime = Cancellation::new(move || Instant::now() < deadline);
    let NativeStereoSelection::Qualified(source) =
        NativeStereoSource::select(file, lifetime.clone(), &library, plan(3)).unwrap()
    else {
        panic!("trusted runtime should qualify the pinned fixture")
    };
    assert_eq!(source.total_frames(), 88207);
    let prepare = Cancellation::default();
    let NativeStereoStart::Ready(mut pass) = source.start(0, &prepare).unwrap() else {
        panic!("valid fixture must prepare")
    };
    prepare.cancel(); // The completed request no longer owns this playing pass.
    let mut pcm = pass.take_reader().unwrap();
    let first = pass.control();
    first.publish_sink().unwrap();
    first.bind_entry(13).unwrap();
    first.observe(13, Some(0.0), false, true).unwrap();
    let mut sample = [0; 8192];
    pcm.read_exact(&mut sample).unwrap();
    assert_ne!(sample.as_slice(), dry_prefix(0, 1024).as_slice());
    first.observe(13, Some(0.0), true, false).unwrap();
    let off = pass.disable_master().unwrap();
    let on = pass.apply_material(plan(6)).unwrap();
    let off_again = pass.disable_master().unwrap();
    assert!(off_again > on && on > off);
    first
        .wait_applied(off_again, Duration::from_secs(2))
        .unwrap();
    assert_eq!(first.snapshot().unwrap().native_entry, Some(13));
    assert_eq!(first.snapshot().unwrap().native_output_frame, 0);
    drop(pcm);
    let closed = first.snapshot().unwrap();
    assert!(closed.reader_closed && !closed.native_sink_retired);
    first.retire_sink(); // Explicit synthetic sink owner; not mpv verification.
    let mut source = Some(pass.reclaim().unwrap());
    for (target, entry) in [(22050, 14), (0, 15)] {
        let NativeStereoStart::Ready(mut pass) = source
            .take()
            .unwrap()
            .start(target, &Cancellation::default())
            .unwrap()
        else {
            panic!("frozen indexed fixture must prepare a sample seek")
        };
        let mut pcm = pass.take_reader().unwrap();
        let current = pass.control();
        assert_eq!(current.snapshot().unwrap().first_source_frame, target);
        current.publish_sink().unwrap();
        current.bind_entry(entry).unwrap();
        current.observe(entry, Some(0.0), false, true).unwrap();
        let mut post_seek = [0; 4096];
        pcm.read_exact(&mut post_seek).unwrap();
        assert_eq!(post_seek.as_slice(), dry_prefix(target, 512).as_slice());
        assert!(matches!(
            first.observe(entry, Some(0.0), false, false),
            Err(Error::Cancelled)
        ));
        if target == 0 {
            lifetime.cancel();
            post_seek.fill(0xAA);
            assert!(pcm.read(&mut post_seek).is_err());
            assert_eq!(post_seek, [0; 4096]);
            assert!(pass.disable_master().is_err());
            drop(pcm);
            current.retire_sink();
            assert!(matches!(pass.reclaim(), Err(Error::Cancelled)));
        } else {
            drop(pcm);
            current.retire_sink();
            source = Some(pass.reclaim().unwrap());
        }
    }
}

#[test]
#[ignore = "requires the trusted native libFLAC 1.5.0; no audio device"]
fn native_flac_metadata_failure_returns_original_unpublished_reader() {
    let dir = tempfile::tempdir().unwrap();
    let library = super::super::super::sdk_path();
    // Even an available trusted runtime must leave wider compressed blocks
    // to stock, rather than relying on the safe decoded buffer to reject late.
    let mut wide = FLAC.to_vec();
    wide[10..12].copy_from_slice(&4096_u16.to_be_bytes());
    let (file, _) = checked_file(dir.path(), "wide.flac", &wide);
    let NativeStereoSelection::Stock(wide_file) =
        NativeStereoSource::select(file, Cancellation::default(), &library, plan(3)).unwrap()
    else {
        panic!("4096-frame source must remain stock")
    };
    let (mut file, _) = wide_file.into_stock().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, wide);

    // The larger ordinary indexed lane still returns the same held reader
    // for declarations above its independent absolute limit.
    let mut above_limit = FLAC.to_vec();
    above_limit[10..12].copy_from_slice(&4609_u16.to_be_bytes());
    let (file, _) = checked_file(dir.path(), "above-limit.flac", &above_limit);
    let NativeStereoSelection::Stock(reader) =
        NativeStereoSource::select_indexed(file, Cancellation::default(), &library, plan(3))
            .unwrap()
    else {
        panic!("4609-frame indexed source must remain stock")
    };
    let (mut file, _) = reader.into_stock().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, above_limit);

    // The valid STREAMINFO is not marked as the last metadata block. With
    // its following block omitted, the actual decoder must fail to prepare.
    let bytes = &FLAC[..42];
    assert_eq!(bytes[4] & 128, 0);
    let (file, _) = checked_file(dir.path(), "truncated.flac", bytes);
    let NativeStereoSelection::Qualified(source) =
        NativeStereoSource::select(file, Cancellation::default(), &library, plan(3)).unwrap()
    else {
        panic!("the STREAMINFO alone is in scope")
    };
    let NativeStereoStart::Stock(source) = source.start(0, &Cancellation::default()).unwrap()
    else {
        panic!("truncated original metadata must not publish an active pass")
    };
    let (mut file, authority) = source.into_reader().unwrap().into_stock().unwrap();
    authority.check().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
}
