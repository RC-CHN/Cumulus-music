use cumulus_core::{
    Error,
    library::probe::{AudioProbe, Ffprobe},
};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::Duration,
};

#[test]
fn held_probe_restores_offset_when_the_child_cannot_start() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("song.flac");
    fs::write(&source, b"fLaCsynthetic").unwrap();
    let mut held = File::open(source).unwrap();
    held.seek(SeekFrom::Start(6)).unwrap();
    let probe = Ffprobe::new(
        root.path().join("missing-ffprobe.exe"),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(matches!(
        probe.inspect_held_flac(&mut held, OsStr::new("song.flac")),
        Err(Error::Io(_))
    ));
    assert_eq!(held.stream_position().unwrap(), 6);
    assert!(matches!(
        probe.inspect_held_flac(&mut held, OsStr::new("sub/song.flac")),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(held.stream_position().unwrap(), 6);
}

#[cfg(unix)]
#[test]
fn held_probe_passes_only_the_fixed_fd_input_and_restores_after_child_failure() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("song.flac");
    fs::write(&source, b"fLaC\nheld").unwrap();
    let mut held = File::open(&source).unwrap();
    fs::write(root.path().join("replacement"), b"fLaC\nnew").unwrap();
    fs::rename(root.path().join("replacement"), &source).unwrap();
    let executable = root.path().join("probe");
    let body = r#"#!/bin/sh
set -eu
test "$#" = 12 && test "$3" = -protocol_whitelist && test "$4" = fd || exit 2
shift 8
test "$1 $2 $3 $4" = '-f flac -i fd:' || exit 2
IFS= read -r marker
test "$marker" = fLaC && test "$(cat)" = held || exit 2
"#;
    let probe = Ffprobe::new(executable.clone(), Duration::from_secs(1)).unwrap();
    let logical = OsStr::new("filename fallback.FLAC");
    fs::write(
        &executable,
        format!(
            "{body}printf '%s' '{{\"streams\":[{{\"codec_type\":\"audio\",\"codec_name\":\"flac\"}}]}}'\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    held.seek(SeekFrom::Start(6)).unwrap();
    let view = probe.inspect_held_flac(&mut held, logical).unwrap();
    assert_eq!(view.title, "filename fallback");
    assert_eq!(held.stream_position().unwrap(), 6);

    fs::write(&executable, format!("{body}exit 4\n")).unwrap();
    assert!(matches!(
        probe.inspect_held_flac(&mut held, logical),
        Err(Error::Media(_))
    ));
    assert_eq!(held.stream_position().unwrap(), 6);
}

#[test]
#[ignore = "set CUMULUS_HELD_FFPROBE to the absolute path of the ffprobe being verified"]
fn actual_ffprobe_held_flac_matches_path_metadata_and_never_reopens() {
    const TAGGED: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");
    const UNTAGGED: &[u8] = include_bytes!("fixtures/stereo-pcm-source.flac");
    let executable = PathBuf::from(
        std::env::var_os("CUMULUS_HELD_FFPROBE").expect("explicit ffprobe under test"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let probe = Ffprobe::new(executable, Duration::from_secs(5)).unwrap();
    for (bytes, replacement, title) in [
        (TAGGED, UNTAGGED, "Old title"),
        (UNTAGGED, TAGGED, "合成; 音乐"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let logical = OsStr::new("合成; 音乐.FLAC");
        let source = root.path().join(logical);
        fs::write(&source, bytes).unwrap();
        let expected = probe.inspect(&source).unwrap();
        assert_eq!(expected.title, title);
        assert!(expected.duration > 0.0 && expected.bitrate.is_some());

        let mut held = File::open(&source).unwrap();
        held.seek(SeekFrom::Start(11)).unwrap();
        assert_eq!(
            probe.inspect_held_flac(&mut held, logical).unwrap(),
            expected
        );
        assert_eq!(held.stream_position().unwrap(), 11);

        // Change the pathname while the original file and cursor remain held.
        fs::rename(&source, root.path().join("held.flac")).unwrap();
        fs::write(&source, replacement).unwrap();
        assert_ne!(probe.inspect(&source).unwrap(), expected);
        assert_eq!(
            probe.inspect_held_flac(&mut held, logical).unwrap(),
            expected
        );
        assert_eq!(held.stream_position().unwrap(), 11);
        let mut actual = [0; 8];
        held.read_exact(&mut actual).unwrap();
        assert_eq!(actual, bytes[11..19]);

        let damaged = root.path().join("damaged");
        fs::write(&damaged, b"fLaCdamaged synthetic metadata").unwrap();
        let mut invalid = File::open(damaged).unwrap();
        invalid.seek(SeekFrom::Start(9)).unwrap();
        assert!(matches!(
            probe.inspect_held_flac(&mut invalid, logical),
            Err(Error::Media(_))
        ));
        assert_eq!(invalid.stream_position().unwrap(), 9);
    }
}
