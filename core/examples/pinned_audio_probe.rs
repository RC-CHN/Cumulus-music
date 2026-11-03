//! Explicit real libmpv/null probe using synthetic local bytes only.
use cumulus_core::{
    Error,
    media::ncm::{NcmWriteOptions, write_stream},
    playback::engine::{AudioEngine, Cancellation, LoadRequest, MediaInput, MpvConfig, MpvEngine},
};
use std::{
    fs::{self, File},
    io::Cursor,
    thread,
    time::{Duration, Instant},
};

fn main() {
    let root = tempfile::tempdir().unwrap();
    let seed = include_bytes!("../tests/fixtures/tag_audio/seed.flac");
    let path = root.path().join("fixture.flac");
    fs::write(&path, seed).unwrap();
    let opened = File::open(&path).unwrap();
    let moved = root.path().join("moved.flac");
    fs::rename(&path, &moved).unwrap();
    fs::write(&path, b"invalid audio replacement").unwrap();

    let mut engine: Box<dyn AudioEngine> = Box::new(
        MpvEngine::open(MpvConfig {
            null_output: true,
            ..Default::default()
        })
        .unwrap(),
    );
    let authority = Cancellation::default();
    let loaded = engine
        .load_pinned_audio(opened, authority.clone(), &Cancellation::default())
        .unwrap();
    assert_eq!(loaded.codec.as_deref(), Some("flac"));
    assert!(loaded.duration.is_some_and(|v| (0.9..1.1).contains(&v)));
    engine.seek(0.3).unwrap();
    engine.pause(false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let current = engine.poll().unwrap();
        if current.position.is_some_and(|position| position > 0.32) {
            break;
        }
        assert!(Instant::now() < deadline, "pinned playback did not advance");
        thread::sleep(Duration::from_millis(15));
    }
    authority.cancel();
    assert!(matches!(engine.poll(), Err(Error::Cancelled)));
    assert!(engine.poll().unwrap().entry_id.is_none());
    assert!(matches!(
        engine.load_pinned_audio(
            File::open(&path).unwrap(),
            Cancellation::default(),
            &Cancellation::default()
        ),
        Err(Error::InvalidInput(_))
    ));
    probe_ncm(engine.as_mut(), root.path(), seed);

    let next = engine
        .load(
            LoadRequest::new(MediaInput::File(moved)),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(next.codec.as_deref(), Some("flac"));
    engine.stop().unwrap();
    println!("pinned FLAC decoded, position advanced, revocation stopped, ordinary load restored");
}

fn probe_ncm(engine: &mut dyn AudioEngine, root: &std::path::Path, seed: &[u8]) {
    let path = root.join("fixture.ncm");
    let mut container = Vec::new();
    write_stream(
        &mut Cursor::new(seed),
        &mut container,
        NcmWriteOptions {
            metadata: None,
            cover: &[],
            audio_key: Some(b"synthetic audio key"),
        },
    )
    .unwrap();
    fs::write(&path, &container).unwrap();
    let pinned = File::open(&path).unwrap();
    let moved = root.join("moved.ncm");
    fs::rename(&path, &moved).unwrap();
    // Same path and bytes except a header field covered by the CRC.
    container[9] ^= 1;
    fs::write(&path, container).unwrap();
    let authority = Cancellation::default();
    let loaded = engine
        .load_pinned_ncm(pinned, authority.clone(), &Cancellation::default())
        .unwrap();
    assert_eq!(loaded.codec.as_deref(), Some("flac"));
    assert!(loaded.duration.is_some_and(|v| (0.9..1.1).contains(&v)));
    engine.seek(0.3).unwrap();
    engine.pause(false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if engine.poll().unwrap().position.is_some_and(|p| p > 0.32) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "pinned NCM playback did not advance"
        );
        thread::sleep(Duration::from_millis(15));
    }
    authority.cancel();
    assert!(matches!(engine.poll(), Err(Error::Cancelled)));
    assert!(engine.poll().unwrap().entry_id.is_none());
    assert!(matches!(
        engine.load_pinned_ncm(
            File::open(&path).unwrap(),
            Cancellation::default(),
            &Cancellation::default(),
        ),
        Err(Error::Media("NCM header version or CRC mismatch"))
    ));
    assert!(matches!(
        engine.load_pinned_ncm(
            File::open(moved).unwrap(),
            authority,
            &Cancellation::default()
        ),
        Err(Error::Cancelled)
    ));
    println!(
        "pinned strict NCM/FLAC decoded and sought, revoked authority stopped it, bad CRC rejected"
    );
}
