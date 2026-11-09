use cumulus_core::{Error, library::probe::Ffprobe};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

const TAGGED: &[u8] = include_bytes!("fixtures/tag_audio/seed.mp3");
const STEREO: &[u8] = include_bytes!("fixtures/held_audio/stereo-44100.mp3");
// Frozen leading ID3 extent and first two MPEG packet boundaries from the
// generated seed. The raw slice retains Info/Xing and the original audio.
const FIRST_FRAME: usize = 1404;
const FIRST_AUDIO: usize = 1620;
const AUDIO_BYTES: usize = 72;
const LABEL: &str = "合成; music.MP3";

fn held(path: &Path) -> File {
    #[cfg(windows)]
    {
        cumulus_core::library::identity::windows::WindowsFileIdentity::open_local_fixed_ntfs_nofollow(path)
            .unwrap().expect("test temp root is fixed local NTFS").0
    }
    #[cfg(not(windows))]
    {
        File::open(path).unwrap()
    }
}

fn probe_at(executable: PathBuf) -> Ffprobe {
    Ffprobe::new(executable, Duration::from_secs(5)).unwrap()
}

#[test]
fn held_mp3_rejects_malformed_prefixes_before_starting_ffprobe() {
    let root = tempfile::tempdir().unwrap();
    let probe = probe_at(root.path().join("missing-ffprobe.exe"));
    let path = root.path().join(LABEL);
    let mut oversized_tag = TAGGED.to_vec();
    oversized_tag[6..10].copy_from_slice(&[8, 0, 0, 1]); // 16 MiB + 1
    let mut invalid_synchsafe = TAGGED.to_vec();
    invalid_synchsafe[7] |= 0x80;
    let mut wrong_layer = TAGGED[FIRST_FRAME..].to_vec();
    wrong_layer[1] = 0xe5; // MPEG 2.5 Layer II, not Layer III.
    let mut misplaced_frame = b"ID3\x04\x00\x00\x00\x00\x00\x00arbitrary junk".to_vec();
    misplaced_frame.extend(&TAGGED[FIRST_FRAME..]);
    let cases = [
        (
            "wrong container",
            include_bytes!("fixtures/tag_audio/seed.flac").as_slice(),
        ),
        ("oversized tag", oversized_tag.as_slice()),
        ("non-synchsafe tag", invalid_synchsafe.as_slice()),
        ("wrong MPEG layer", wrong_layer.as_slice()),
        ("missing leading frame", misplaced_frame.as_slice()),
        ("truncated second frame", &TAGGED[..FIRST_AUDIO + 11]),
    ];
    for (case, bytes) in cases {
        fs::write(&path, bytes).unwrap();
        let mut file = File::open(&path).unwrap();
        file.seek(SeekFrom::Start(6)).unwrap();
        assert!(
            matches!(
                probe.inspect_held_mp3(&mut file, OsStr::new(LABEL)),
                Err(Error::Media(_))
            ),
            "{case} reached the unavailable child or was admitted"
        );
        assert_eq!(file.stream_position().unwrap(), 6, "{case}");
    }
    // A valid prefix is the contrasting control: it does reach child spawn.
    fs::write(&path, TAGGED).unwrap();
    let mut file = File::open(&path).unwrap();
    file.seek(SeekFrom::Start(6)).unwrap();
    assert!(matches!(
        probe.inspect_held_mp3(&mut file, OsStr::new(LABEL)),
        Err(Error::Io(_))
    ));
    assert_eq!(file.stream_position().unwrap(), 6);
    assert!(matches!(
        probe.inspect_held_mp3(&mut file, OsStr::new("sub/song.mp3")),
        Err(Error::InvalidInput(_))
    ));
}

#[cfg(unix)]
#[test]
fn held_mp3_refuses_oversized_file_before_opening_child() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(LABEL);
    fs::write(&path, TAGGED).unwrap();
    let writable = fs::OpenOptions::new().write(true).open(&path).unwrap();
    writable.set_len(256 * 1024 * 1024 + 1).unwrap();
    drop(writable);
    let mut file = held(&path);
    file.seek(SeekFrom::Start(12)).unwrap();
    let probe = probe_at(root.path().join("missing-ffprobe"));
    assert!(matches!(
        probe.inspect_held_mp3(&mut file, OsStr::new(LABEL)),
        Err(Error::Media("held MP3 exceeds regular file bounds"))
    ));
    assert_eq!(file.stream_position().unwrap(), 12);
}

#[cfg(unix)]
#[test]
fn clean_child_summary_still_requires_mp3_container_codec_and_decoded_audio() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(LABEL);
    fs::write(&path, TAGGED).unwrap();
    let executable = root.path().join("synthetic-probe");
    let probe = probe_at(executable.clone());
    let summary = serde_json::json!({
        "format": {"format_name":"mp3","duration":"1.0"},
        "streams": [{"index":0,"codec_type":"audio","codec_name":"mp3","sample_rate":"8000","channels":1}],
        "frames": [{"media_type":"audio","stream_index":0,"nb_samples":576,"channels":1}]
    });
    for (parts, value, expected) in [
        (
            &["format", "format_name"][..],
            "wav".into(),
            "held audio was not decoded as MP3",
        ),
        (
            &["codec_name"][..],
            "mp2".into(),
            "held audio was not decoded as MP3",
        ),
        (
            &["frames"][..],
            serde_json::json!([]),
            "held MP3 produced no bounded audio frames",
        ),
    ] {
        let mut answer = summary.clone();
        match parts {
            ["format", name] => answer["format"][*name] = value,
            ["codec_name"] => answer["streams"][0]["codec_name"] = value,
            ["frames"] => answer["frames"] = value,
            _ => unreachable!(),
        }
        fs::write(&executable, format!("#!/bin/sh\nprintf '%s' '{answer}'\n")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let mut file = held(&path);
        file.seek(SeekFrom::Start(11)).unwrap();
        assert!(matches!(
            probe.inspect_held_mp3(&mut file, OsStr::new(LABEL)),
            Err(Error::Media(message)) if message == expected
        ));
        assert_eq!(file.stream_position().unwrap(), 11);
    }
}

#[test]
#[ignore = "set CUMULUS_HELD_FFPROBE to the absolute path of the ffprobe being verified"]
fn actual_ffprobe_decodes_only_the_held_mp3_and_rejects_corrupt_audio() {
    let executable = PathBuf::from(
        std::env::var_os("CUMULUS_HELD_FFPROBE").expect("explicit ffprobe under test"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let probe = probe_at(executable);
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join(LABEL);
    fs::write(&source, TAGGED).unwrap();
    let mut original = held(&source);
    original.seek(SeekFrom::Start(11)).unwrap();
    fs::rename(&source, root.path().join("old-held.mp3")).unwrap();
    fs::write(&source, &TAGGED[FIRST_FRAME..]).unwrap();
    let mut replacement = held(&source);
    let old = probe
        .inspect_held_mp3(&mut original, OsStr::new(LABEL))
        .unwrap();
    let new = probe
        .inspect_held_mp3(&mut replacement, OsStr::new(LABEL))
        .unwrap();
    assert_eq!(old.title, "Old title");
    assert_eq!(new.title, "合成; music");
    for metadata in [old, new] {
        assert_eq!(metadata.container, "mp3");
        assert_eq!(metadata.format.as_deref(), Some("mp3"));
        assert_eq!(metadata.duration, 1.0);
        assert_eq!(metadata.sample_rate, Some(8000));
        assert_eq!(metadata.channels, Some(1));
    }
    assert_eq!(original.stream_position().unwrap(), 11);
    let mut bytes = [0; 8];
    original.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, TAGGED[11..19]);
    drop(replacement);

    let stereo_path = root.path().join("stereo.mp3");
    fs::write(&stereo_path, STEREO).unwrap();
    let stereo = probe
        .inspect_held_mp3(&mut held(&stereo_path), OsStr::new("stereo.mp3"))
        .unwrap();
    assert_eq!(stereo.sample_rate, Some(44_100));
    assert_eq!(stereo.channels, Some(2));
    assert!((127_900..=128_100).contains(&stereo.bitrate.unwrap()));
    assert!((0.548..0.550).contains(&stereo.duration));

    let mut corrupt = TAGGED.to_vec();
    // Corrupt one real audio packet without changing either leading header
    // or the Info frame. Other packets are still good; metadata alone can
    // report a valid MP3 even though the decoder reports an error.
    corrupt[FIRST_AUDIO + 4..FIRST_AUDIO + AUDIO_BYTES].fill(0xff);
    let mut header_spoof = Vec::new();
    for _ in 0..6 {
        // MPEG-1 Layer III, 128 kbit/s at 44.1 kHz: 417-byte frame.
        header_spoof.extend([0xff, 0xfb, 0x90, 0x00]);
        header_spoof.extend([0xff; 413]);
    }
    for (name, bytes) in [("bad packet", corrupt), ("header spoof", header_spoof)] {
        fs::write(&source, bytes).unwrap();
        let mut file = held(&source);
        file.seek(SeekFrom::Start(6)).unwrap();
        assert!(
            matches!(
                probe.inspect_held_mp3(&mut file, OsStr::new(LABEL)),
                Err(Error::Media("audio metadata could not be decoded"))
            ),
            "{name} must fail decoder validation, not merely its filename"
        );
        assert_eq!(file.stream_position().unwrap(), 6);
    }
}
