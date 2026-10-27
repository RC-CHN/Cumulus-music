//! Explicit isolated local-library verification using real FFprobe and NCM.
//! All writes stay under a newly created output directory; input is read-only.
use cumulus_core::{
    Error, Result,
    library::{ListOptions, LocalLibrary, LocalResource, LocalRevision, monitor::LibraryMonitor},
    playback::PlaybackQueue,
    storage::StateStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
fn wav(path: &Path, seconds: u32) -> Result<()> {
    let bytes = seconds * 8000 * 2;
    let mut file = File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(bytes + 36).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&8000u32.to_le_bytes())?;
    file.write_all(&16000u32.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    file.write_all(&vec![0; bytes as usize])?;
    Ok(())
}
fn require(value: bool, what: &'static str) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err(Error::State(what))
    }
}

fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: library_roundtrip <reference.ncm> <new-output-directory>",
        ));
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let original_hash = hash(&input)?;
    fs::create_dir(&output)?;
    let music = output.join("music");
    fs::create_dir(&music)?;
    let audio = music.join("sample.wav");
    let ncm = music.join("sample.ncm");
    wav(&audio, 5)?;
    fs::copy(&input, &ncm)?;
    fs::write(
        music.join("wav.cue"),
        "TITLE Synthetic\nFILE sample.wav WAVE\nTRACK 01 AUDIO\nTITLE \"WAV One\"\nINDEX 01 00:01:00\nTRACK 02 AUDIO\nTITLE \"WAV Two\"\nINDEX 01 00:03:37\n",
    )?;
    fs::write(
        music.join("ncm.cue"),
        "TITLE Synthetic\nFILE sample.ncm WAVE\nTRACK 01 AUDIO\nTITLE \"NCM One\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"NCM Two\"\nINDEX 01 00:01:00\n",
    )?;
    let store = Arc::new(StateStore::open(output.join("state"))?);
    let library = LocalLibrary::new(store.clone())?;
    let scan = library.scan(&music)?;
    require(
        scan.errors.is_empty() && scan.updated == 2 && scan.cue_updated == 2,
        "initial real media scan failed",
    )?;
    let visible = library.list(ListOptions::default())?;
    require(visible.len() == 4, "virtual tracks are incomplete")?;
    require(
        library
            .list(ListOptions {
                include_containers: true,
                ..Default::default()
            })?
            .len()
            == 6,
        "container visibility is wrong",
    )?;
    let scan = library.scan(&music)?;
    require(
        scan.unchanged == 2 && scan.cue_unchanged == 2,
        "incremental scan reprobed unchanged sources",
    )?;
    let wav_item = visible
        .iter()
        .find(|i| i.metadata.title == "WAV One")
        .ok_or(Error::State("missing WAV track"))?;
    let resource = library.resource(&wav_item.id)?;
    let mut queue = PlaybackQueue::new(Some(store.clone()), "local")?;
    queue.replace(
        vec![serde_json::to_value(&resource)?],
        0,
        json!({"kind":"local"}),
    )?;
    let reopened_store = Arc::new(StateStore::open(store.directory())?);
    let reopened = LocalLibrary::new(reopened_store.clone())?;
    let queue = PlaybackQueue::new(Some(reopened_store), "local")?;
    let selected = queue
        .selected()
        .ok_or(Error::State("queue did not restore"))?;
    let restored: LocalResource = serde_json::from_value(selected.resource)?;
    let source = reopened.resolve(&restored)?;
    require(
        source.segment.as_ref().is_some_and(|s| s.start == 1.0),
        "CUE start was not restored",
    )?;
    let ncm_item = visible
        .iter()
        .find(|i| i.metadata.title == "NCM Two")
        .ok_or(Error::State("missing NCM track"))?;
    let ncm_source = library.resolve(&library.resource(&ncm_item.id)?)?;
    let decoded_hash = hash(&ncm_source.source)?;
    require(
        ncm_source
            .segment
            .as_ref()
            .is_some_and(|s| s.start == 1.0 && s.duration == 3.0),
        "NCM CUE segment mismatch",
    )?;
    fs::write(&ncm_source.source, b"damaged managed cache")?;
    let rebuilt = library.playable_path(&ncm)?;
    require(
        hash(&rebuilt)? == decoded_hash,
        "managed cache did not repair",
    )?;
    wav(&audio, 6)?;
    require(
        reopened.resolve(&restored).is_err(),
        "old queue accepted changed audio",
    )?;
    let scan = library.scan(&music)?;
    require(
        scan.updated == 1 && scan.cue_updated == 1,
        "dependent CUE did not rescan",
    )?;
    require(
        reopened.resolve(&restored).is_err(),
        "old queue silently accepted a new revision",
    )?;
    let refreshed = library.resource(&wav_item.id)?;
    require(
        matches!(refreshed.local_revision, LocalRevision::Cue { .. }),
        "virtual identity was lost",
    )?;
    library.resolve(&refreshed)?;
    let monitor = LibraryMonitor::new(library.clone(), Duration::from_millis(100))?;
    let root = monitor.watch(&music)?;
    monitor.start()?;
    let added = music.join("added.wav");
    wav(&added, 2)?;
    let start = Instant::now();
    loop {
        if library.list(ListOptions::default())?.len() == 5 {
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            return Err(Error::State("monitor did not import addition"));
        }
        thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(&added)?;
    let start = Instant::now();
    loop {
        if library.list(ListOptions::default())?.len() == 4 {
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            return Err(Error::State("monitor did not mark removal"));
        }
        thread::sleep(Duration::from_millis(20));
    }
    monitor.unwatch(&root.id)?;
    monitor.stop()?;
    library.remove(&wav_item.id)?;
    require(
        audio.exists() && music.join("wav.cue").exists(),
        "removing index deleted source",
    )?;
    require(
        hash(&input)? == original_hash && hash(&ncm)? == original_hash,
        "input NCM changed",
    )?;
    Ok(
        json!({"schema":1,"implementation":"rust-core","checks":{"initial_audio_files":2,"initial_cue_sheets":2,"virtual_tracks":4,"incremental_skip":true,"queue_reopened":true,"cue_source_restored":true,"ncm_decoded_and_segmented":true,"cache_repaired":true,"old_revision_rejected_before_and_after_scan":true,"dependent_cue_rescanned":true,"monitor_add_remove":true,"unwatch_keeps_media":true,"index_remove_keeps_media":true,"input_unchanged":true},"decoded_audio_sha256":decoded_hash,"output_directory":output}),
    )
}
fn main() {
    match run() {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).expect("report")),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
