use cumulus_core::media::tags::{TagPlan, decode_comment, track_plan, write_tagged_copy};
use lofty::{
    config::ParseOptions,
    file::{AudioFile, TaggedFileExt},
    flac::FlacFile,
    mp4::Mp4File,
    mpeg::MpegFile,
    ogg::OggPictureStorage,
    picture::PictureType,
    probe::Probe,
    tag::{Accessor, ItemKey},
};
use serde_json::json;
use std::{fs, io::Cursor};
const PNG: &[u8] = include_bytes!("fixtures/tag_audio/cover.png");
const JPEG: &[u8] = include_bytes!("fixtures/tag_audio/cover.jpg");
const SEEDS: [(&str, &[u8]); 3] = [
    ("mp3", include_bytes!("fixtures/tag_audio/seed.mp3")),
    ("flac", include_bytes!("fixtures/tag_audio/seed.flac")),
    ("m4a", include_bytes!("fixtures/tag_audio/seed.m4a")),
];
fn plan(ext: &str) -> TagPlan {
    track_plan(&json!({"id":123,"name":" 新标题 😀 ","album":{"id":2,"name":" 专辑 "},"artists":[{"name":"歌手甲"},{"name":"歌手乙"}],"position":"2/12","cd":"1/2"}),&json!({"type":ext,"md5":"ab".repeat(16),"br":128000}),false).unwrap()
}

#[test]
fn three_containers_roundtrip_unicode_comments_covers_and_unrelated_metadata() {
    let root = tempfile::tempdir().unwrap();
    for (ext, seed) in SEEDS {
        let source = root.path().join(format!("source.{ext}"));
        fs::write(&source, seed).unwrap();
        let plan = plan(ext);
        for (image, cover) in [("png", PNG), ("jpg", JPEG)] {
            let target = root.path().join(format!("{image}.{ext}"));
            let report = write_tagged_copy(&source, &target, &plan, Some(cover)).unwrap();
            assert!(report.cover);
            let data = fs::read(&target).unwrap();
            assert_eq!(report.bytes, data.len() as u64);
            let file = Probe::new(Cursor::new(&data))
                .guess_file_type()
                .unwrap()
                .read()
                .unwrap();
            let tag = file.primary_tag().unwrap();
            assert_eq!(tag.title().as_deref(), Some("新标题 😀"));
            assert_eq!(tag.artist().as_deref(), Some("歌手甲/歌手乙"));
            assert_eq!(tag.album().as_deref(), Some(" 专辑 "));
            assert_eq!(
                tag.get_string(ItemKey::Lyrics)
                    .or_else(|| tag.get_string(ItemKey::UnsyncLyrics)),
                Some("[00:00.00]保留歌词"),
                "{ext}"
            );
            match ext {
                "mp3" => {
                    let file =
                        MpegFile::read_from(&mut Cursor::new(&data), ParseOptions::new()).unwrap();
                    let tag = file.id3v2().unwrap();
                    let comm = tag
                        .comments()
                        .find(|c| c.language == *b"eng" && c.description.is_empty())
                        .unwrap();
                    assert_eq!(decode_comment(&comm.content).unwrap(), plan.comment_plain);
                    assert!(tag.iter().any(|frame|matches!(frame,lofty::id3::v2::Frame::Comment(c) if c.description=="user" && c.content=="保留评论")));
                    assert_eq!(tag.get_user_text("CUMULUS_TEST"), Some("keep me"));
                }
                "flac" => {
                    let file =
                        FlacFile::read_from(&mut Cursor::new(&data), ParseOptions::new()).unwrap();
                    let tag = file.vorbis_comments().unwrap();
                    assert_eq!(tag.get("CUMULUS_TEST"), Some("keep me"));
                    assert_eq!(
                        decode_comment(tag.get("DESCRIPTION").unwrap()).unwrap(),
                        plan.comment_plain
                    );
                    assert_eq!(file.pictures().len(), 2);
                }
                _ => {
                    let file =
                        Mp4File::read_from(&mut Cursor::new(&data), ParseOptions::new()).unwrap();
                    let tag = file.ilst().unwrap();
                    assert_eq!(
                        (tag.track(), tag.track_total(), tag.disk(), tag.disk_total()),
                        (Some(2), Some(12), Some(1), Some(2))
                    );
                    assert_eq!(
                        decode_comment(tag.comment().unwrap().as_ref()).unwrap(),
                        plan.comment_plain
                    );
                    assert_eq!(tag.pictures().unwrap().count(), 1);
                }
            }
            let pics = tag.pictures();
            let expected_count = if ext == "m4a" { 1 } else { 2 };
            assert_eq!(pics.len(), expected_count);
            assert!(pics.iter().any(|p| p.data() == cover));
            if ext != "m4a" {
                assert!(
                    pics.iter()
                        .any(|p| p.pic_type() == PictureType::CoverBack && p.data() == PNG)
                );
            }
            let repeated = root.path().join(format!("repeat-{image}.{ext}"));
            write_tagged_copy(&target, &repeated, &plan, Some(cover)).unwrap();
            let after = lofty::read_from_path(&repeated).unwrap();
            assert_eq!(
                after.primary_tag().unwrap().pictures().len(),
                expected_count
            );
        }
        assert_eq!(fs::read(source).unwrap(), seed);
    }
}

#[test]
fn failed_postprocessing_never_publishes_or_overwrites_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, SEEDS[0].1).unwrap();
    let target = root.path().join("output");
    let mut plan = plan("mp3");
    plan.encrypted = true;
    assert!(write_tagged_copy(&source, &target, &plan, Some(PNG)).is_err());
    plan.encrypted = false;
    assert!(write_tagged_copy(&source, &target, &plan, Some(b"broken")).is_err());
    assert!(!target.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    fs::write(&target, b"my file").unwrap();
    assert!(write_tagged_copy(&source, &target, &plan, None).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"my file");
    assert!(write_tagged_copy(&source, &source, &plan, None).is_err());
    assert_eq!(fs::read(&source).unwrap(), SEEDS[0].1);
    let invalid = root.path().join("invalid");
    fs::write(&invalid, b"not audio").unwrap();
    let failed = root.path().join("failed");
    assert!(write_tagged_copy(&invalid, &failed, &plan, None).is_err());
    assert!(!failed.exists());
    let mp4 = root.path().join("mp4");
    fs::write(&mp4, SEEDS[2].1).unwrap();
    plan.tags.trck = "65536".into();
    assert!(write_tagged_copy(&mp4, &failed, &plan, None).is_err());
    assert!(!failed.exists());
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".cumulus-tags-")
    }));
}
