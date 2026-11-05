use super::*;
use crate::library::{identity::fingerprint, native_open::checked_audio_file};
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
    assert!(sample.iter().any(|byte| *byte != 0));
    first.observe(13, Some(0.0), true, false).unwrap();
    let off = pass.disable_master().unwrap();
    let on = pass.apply_material(plan(6)).unwrap();
    assert!(on > off);
    first.wait_applied(on, Duration::from_secs(2)).unwrap();
    assert_eq!(first.snapshot().unwrap().native_entry, Some(13));
    assert_eq!(first.snapshot().unwrap().native_output_frame, 0);
    drop(pcm);
    let closed = first.snapshot().unwrap();
    assert!(closed.reader_closed && !closed.native_sink_retired);
    first.retire_sink(); // Explicit synthetic sink owner; not mpv verification.
    let source = pass.reclaim().unwrap();
    let NativeStereoStart::Ready(mut pass) = source.start(22050, &Cancellation::default()).unwrap()
    else {
        panic!("frozen indexed fixture must prepare a sample seek")
    };
    let mut pcm = pass.take_reader().unwrap();
    let second = pass.control();
    assert_eq!(second.snapshot().unwrap().first_source_frame, 22050);
    second.publish_sink().unwrap();
    second.bind_entry(14).unwrap();
    second.observe(14, Some(0.0), false, true).unwrap();
    let mut post_seek = [0; 4096];
    pcm.read_exact(&mut post_seek).unwrap();
    assert_ne!(&sample[..post_seek.len()], &post_seek);
    assert!(matches!(
        first.observe(14, Some(0.0), false, false),
        Err(Error::Cancelled)
    ));
    lifetime.cancel();
    post_seek.fill(0xAA);
    assert!(pcm.read(&mut post_seek).is_err());
    assert_eq!(post_seek, [0; 4096]);
    assert!(pass.disable_master().is_err());
    drop(pcm);
    second.retire_sink();
    assert!(matches!(pass.reclaim(), Err(Error::Cancelled)));
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
