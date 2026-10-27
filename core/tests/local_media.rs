use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::{
    Result,
    library::{
        AudioMetadata,
        cue::parse_cue,
        probe::{AudioProbe, Ffprobe, parse_ffprobe_response},
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

struct Probe {
    durations: BTreeMap<PathBuf, f64>,
    calls: Mutex<Vec<PathBuf>>,
}
impl AudioProbe for Probe {
    fn inspect(&self, path: &Path) -> Result<AudioMetadata> {
        self.calls.lock().unwrap().push(path.into());
        Ok(AudioMetadata {
            format: Some("pcm_s16le".into()),
            duration: self.durations[path],
            title: "synthetic".into(),
            album: String::new(),
            artists: vec![],
            bitrate: None,
            sample_rate: Some(8000),
            channels: Some(1),
            container: "wav".into(),
            cover_bytes: None,
        })
    }
}

#[test]
fn cue_matches_twelve_recovered_multifile_encoding_and_frame_cases() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cue.json")).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let mut durations = BTreeMap::new();
    for (name, duration) in fixture["durations"].as_object().unwrap() {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"synthetic").unwrap();
        durations.insert(path, duration.as_f64().unwrap());
    }
    let probe = Probe {
        durations,
        calls: Mutex::new(vec![]),
    };
    for row in fixture["cases"].as_array().unwrap() {
        let cue = root.join("sheet.cue");
        std::fs::write(
            &cue,
            STANDARD.decode(row["data"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        probe.calls.lock().unwrap().clear();
        let sheet = parse_cue(cue, &probe).unwrap();
        assert_eq!(sheet.sources.len(), probe.calls.lock().unwrap().len());
        assert_eq!(
            sheet.tracks.len(),
            row["expected"].as_array().unwrap().len()
        );
        for (actual, expected) in sheet.tracks.iter().zip(row["expected"].as_array().unwrap()) {
            assert_eq!(
                actual.path.strip_prefix(&root).unwrap(),
                Path::new(expected["path"].as_str().unwrap())
            );
            assert_eq!(actual.track, expected["track"].as_u64().unwrap() as u8);
            assert_eq!(actual.title, expected["title"]);
            assert_eq!(actual.album, expected["album"]);
            assert_eq!(actual.artist, expected["artist"]);
            assert_eq!(actual.offset, expected["offset"].as_f64().unwrap());
            assert_eq!(actual.duration, expected["duration"].as_f64().unwrap());
        }
    }
}

#[test]
fn cue_rejects_missing_duplicate_out_of_range_mixed_disc_and_bad_text() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let audio = root.join("a.wav");
    std::fs::write(&audio, []).unwrap();
    let mut probe = Probe {
        durations: BTreeMap::from([(audio.clone(), 3.0)]),
        calls: Mutex::new(vec![]),
    };
    let cue = root.join("sheet.cue");
    for text in [
        "",
        "TRACK 01 AUDIO\nINDEX 01 00:00:00",
        "FILE a.wav WAVE\nTRACK 01 AUDIO",
        "FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:75",
        "FILE a.wav WAVE\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00",
        "FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nTRACK 01 AUDIO\nINDEX 01 00:01:00",
        "FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:03:00",
        "FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\nINDEX 01 00:01:00",
        "FILE \"a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00",
        "FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:02:00\nTRACK 02 AUDIO\nINDEX 01 00:01:00",
    ] {
        std::fs::write(&cue, text).unwrap();
        assert!(parse_cue(&cue, &probe).is_err(), "accepted {text}");
    }
    for bytes in [vec![0xff, 0xfe, 0], vec![b'a'; 4 * 1024 * 1024 + 1]] {
        std::fs::write(&cue, bytes).unwrap();
        assert!(parse_cue(&cue, &probe).is_err());
    }
    std::fs::write(&cue, "FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00").unwrap();
    for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        probe.durations.insert(audio.clone(), duration);
        assert!(parse_cue(&cue, &probe).is_err());
    }
}

#[test]
fn cue_accepts_explicit_parent_and_absolute_local_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let audio = root.join("a.wav");
    std::fs::write(&audio, []).unwrap();
    let probe = Probe {
        durations: BTreeMap::from([(audio.clone(), 3.0)]),
        calls: Mutex::new(vec![]),
    };
    std::fs::create_dir(root.join("sheets")).unwrap();
    let cue = root.join("sheets/one.cue");
    for file in [
        "../a.wav".into(),
        audio.to_str().unwrap().replace('\\', "/"),
    ] {
        std::fs::write(
            &cue,
            format!("FILE \"{file}\" WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00"),
        )
        .unwrap();
        assert_eq!(parse_cue(&cue, &probe).unwrap().tracks[0].path, audio);
    }
}

#[test]
fn probe_metadata_uses_audio_stream_and_strict_finite_facts() {
    let response = json!({"streams":[{"codec_type":"video","codec_name":"png"},{"codec_type":"audio","codec_name":"flac","sample_rate":"44100","channels":2}],"format":{"duration":"3.25","bit_rate":"900000","tags":{"TITLE":"标题","ARTIST":"作者","Album":"专辑"}}});
    let facts = parse_ffprobe_response(Path::new("example.FLAC"), &response).unwrap();
    assert_eq!(facts.format.as_deref(), Some("flac"));
    assert_eq!(facts.duration, 3.25);
    assert_eq!(facts.title, "标题");
    assert_eq!(facts.artists, vec!["作者"]);
    assert_eq!(facts.bitrate, Some(900000));
    assert_eq!(facts.container, "flac");
    assert!(
        parse_ffprobe_response(
            Path::new("file"),
            &json!({"streams":[{"codec_type":"video"}]})
        )
        .is_err()
    );
    for value in [json!("NaN"), json!("Infinity"), json!(-1)] {
        let mut raw = response.clone();
        raw["format"]["duration"] = value;
        assert!(parse_ffprobe_response(Path::new("file"), &raw).is_err());
    }
    let missing = parse_ffprobe_response(
        Path::new("fallback.wav"),
        &json!({"streams":[{"codec_type":"audio"}]}),
    )
    .unwrap();
    assert_eq!(missing.title, "fallback");
    assert_eq!(missing.duration, 0.0);
    assert!(Ffprobe::new(PathBuf::new(), std::time::Duration::from_secs(30)).is_err());
}

#[cfg(unix)]
#[test]
fn real_process_boundary_handles_timeout_failure_and_output_limits() {
    use cumulus_core::Error;
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("audio; literal.wav");
    std::fs::write(&source, []).unwrap();
    let executable = tmp.path().join("probe");
    let helper = |body: &str| {
        std::fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    };
    helper(
        "printf '%s' '{\"streams\":[{\"codec_type\":\"audio\",\"codec_name\":\"wav\"}],\"format\":{\"duration\":\"1\"}}'",
    );
    let probe = Ffprobe::new(executable.clone(), Duration::from_secs(1)).unwrap();
    assert_eq!(probe.inspect(&source).unwrap().duration, 1.0);
    helper("exit 1");
    assert!(matches!(probe.inspect(&source), Err(Error::Media(_))));
    helper("exec sleep 3");
    let start = Instant::now();
    assert!(matches!(
        Ffprobe::new(executable.clone(), Duration::from_millis(40))
            .unwrap()
            .inspect(&source),
        Err(Error::State(_))
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    helper("exec yes x");
    assert!(matches!(probe.inspect(&source), Err(Error::Media(_))));
    helper("printf 'not JSON'");
    assert!(matches!(probe.inspect(&source), Err(Error::Media(_))));
}
