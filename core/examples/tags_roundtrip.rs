//! Offline integration entry point. Creates only a new caller-selected folder;
//! all inputs are synthetic tone fixtures. Independent validation uses ffmpeg
//! and Mutagen after this Rust process exits, never as a runtime dependency.
use cumulus_core::{
    Result,
    media::tags::{track_plan, write_tagged_copy},
};
use serde_json::json;
use std::{fs, path::PathBuf};
fn main() -> Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: tags_roundtrip NEW_DIRECTORY");
    fs::create_dir(&root)?;
    let mut reports = Vec::new();
    for (ext, seed) in [
        (
            "mp3",
            include_bytes!("../tests/fixtures/tag_audio/seed.mp3").as_slice(),
        ),
        (
            "flac",
            include_bytes!("../tests/fixtures/tag_audio/seed.flac").as_slice(),
        ),
        (
            "m4a",
            include_bytes!("../tests/fixtures/tag_audio/seed.m4a").as_slice(),
        ),
    ] {
        let source = root.join(format!("source.{ext}"));
        fs::write(&source, seed)?;
        let plan = track_plan(
            &json!({"id":123,"name":" 新标题 😀 ","album":{"id":2,"name":" 专辑 "},"artists":[{"name":"歌手甲"},{"name":"歌手乙"}],"position":"2/12","cd":"1/2"}),
            &json!({"type":ext,"md5":"ab".repeat(16),"br":128000}),
            false,
        )?;
        for (kind, cover) in [
            (
                "png",
                include_bytes!("../tests/fixtures/tag_audio/cover.png").as_slice(),
            ),
            (
                "jpg",
                include_bytes!("../tests/fixtures/tag_audio/cover.jpg").as_slice(),
            ),
        ] {
            let output = root.join(format!("{kind}.{ext}"));
            let report = write_tagged_copy(&source, &output, &plan, Some(cover))?;
            reports.push(json!({"extension":ext,"cover":kind,"report":report,"comment_plain":plan.comment_plain}));
        }
        assert_eq!(fs::read(source)?, seed);
    }
    fs::write(
        root.join("rust-results.json"),
        serde_json::to_vec_pretty(&json!({"cases":reports,"source_unchanged":true}))?,
    )?;
    println!(
        "wrote {} independently verifiable tag outputs",
        reports.len()
    );
    Ok(())
}
