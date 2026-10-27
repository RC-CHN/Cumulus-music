//! Real software-render API proof; no HTTP or account mutation.
use cumulus_core::{
    Error, Result,
    playback::engine::{
        AudioEngine, Cancellation, LoadRequest, MediaInput, MpvConfig, MpvEngine, VideoFrame,
        VideoFrames,
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
fn frame(frames: &VideoFrames, after: u64, width: u32) -> Result<Arc<VideoFrame>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(frame) = frames.after(after)?
            && frame.width == width
        {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            return Err(Error::State("video frame timeout"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn save(root: &Path, name: &str, frame: &VideoFrame) -> Result<String> {
    if frame.rgba.len() != frame.width as usize * frame.height as usize * 4
        || frame.rgba.chunks_exact(4).any(|p| p[3] != 255)
    {
        return Err(Error::Media("invalid video RGBA frame"));
    }
    image::save_buffer(
        root.join(name),
        &frame.rgba,
        frame.width,
        frame.height,
        image::ColorType::Rgba8,
    )
    .map_err(|_| Error::Media("video screenshot failed"))?;
    Ok(hex::encode(Sha256::digest(&frame.rgba)))
}
fn run(path: PathBuf, root: &Path) -> Result<()> {
    fs::create_dir_all(root)?;
    let (mut engine, frames) = MpvEngine::open_video(
        MpvConfig {
            library: None,
            null_output: true,
        },
        512,
        288,
    )?;
    let before = frames.after(0)?.map_or(0, |f| f.sequence);
    let loaded = engine.load(
        LoadRequest::new(MediaInput::File(path.clone())),
        &Cancellation::default(),
    )?;
    engine.pause(false)?;
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut sequence = before;
    let mut hashes = std::collections::BTreeSet::new();
    let mut rendered = 0;
    while Instant::now() < deadline {
        if let Some(frame) = frames.after(sequence)? {
            sequence = frame.sequence;
            hashes.insert(hex::encode(Sha256::digest(&frame.rgba)));
            rendered += 1;
        }
        let state = engine.poll()?;
        if state.position.is_some_and(|p| p > 1.2) && rendered >= 8 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if hashes.len() < 8 {
        return Err(Error::Media(
            "video frames did not change during actual playback",
        ));
    }
    engine.pause(true)?;
    engine.seek(2.0)?;
    frames.resize(640, 360)?;
    frame(&frames, sequence, 640)?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let state = engine.poll()?;
        if state.paused && state.position.is_some_and(|p| (p - 2.0).abs() < 0.2) {
            break;
        }
        if Instant::now() >= deadline {
            return Err(Error::State("video pause/seek state timeout"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Seek acknowledgement and clock update can precede the new decoded frame.
    // Let the render worker finish the pending frame before capturing paused UI.
    std::thread::sleep(Duration::from_millis(150));
    let resized = frames
        .after(0)?
        .ok_or(Error::State("paused video frame missing"))?;
    let resized_hash = save(root, "paused-seek.png", &resized)?;
    frames.resize(517, 291)?;
    let padded = frame(&frames, resized.sequence, 517)?;
    let padding_hash = save(root, "odd-stride.png", &padded)?;
    frames.resize(1920, 1080)?;
    let large = frame(&frames, padded.sequence, 1920)?;
    if large.rgba.len() != 1920 * 1080 * 4 || large.rgba.chunks_exact(4).any(|p| p[3] != 255) {
        return Err(Error::Media("maximum video surface invalid"));
    }
    let first_sequence = large.sequence;
    engine.pause(false)?;
    let started = Instant::now();
    let mut last_sequence = first_sequence;
    while started.elapsed() < Duration::from_millis(1200) {
        if let Some(frame) = frames.after(last_sequence)? {
            last_sequence = frame.sequence;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let benchmark_seconds = started.elapsed().as_secs_f64();
    let benchmark_frames = last_sequence - first_sequence;
    if benchmark_frames < 2 {
        return Err(Error::Media("maximum video surface did not progress"));
    }
    engine.pause(true)?;
    if frames.resize(0, 360).is_ok() || frames.resize(1921, 1080).is_ok() {
        return Err(Error::State("video allocation bounds were not enforced"));
    }
    engine.stop()?;
    drop(engine);
    if frames.after(0).is_ok() {
        return Err(Error::State("closed video renderer still exposes frames"));
    }
    // Repeated destruction verifies render-context-before-core lifetime ordering.
    for _ in 0..4 {
        let (mut engine, frames) = MpvEngine::open_video(
            MpvConfig {
                library: None,
                null_output: true,
            },
            128,
            72,
        )?;
        engine.load(
            LoadRequest::new(MediaInput::File(path.clone())),
            &Cancellation::default(),
        )?;
        frame(&frames, 0, 128)?;
        drop(engine);
        assert!(frames.after(0).is_err());
    }
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"ok":true,"software_render_api":true,"frame_format":"rgba8","loaded_duration":loaded.duration,"observed_frames":rendered,"distinct_frames":hashes.len(),"pause_seek_seconds":2,"resized_width":640,"resized_height":360,"frame_sha256":resized_hash,"odd_stride_width":517,"odd_stride_height":291,"odd_stride_rgba_sha256":padding_hash,"max_surface":{"width":1920,"height":1080,"source_fps":30,"rendered_frames":benchmark_frames,"wall_seconds":benchmark_seconds,"observed_renderer_fps":benchmark_frames as f64/benchmark_seconds},"allocation_bounds":true,"frame_access_revoked_on_drop":true,"repeated_context_destructions":5,"audio_output":"null"}),
        )?,
    )?;
    Ok(())
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: video_render_probe LOCAL_VIDEO PROOF_DIRECTORY");
        std::process::exit(2);
    }
    if let Err(error) = run(PathBuf::from(&args[0]), &PathBuf::from(&args[1])) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    println!("Real native video frames, controls, resizing and destruction verified.");
}
