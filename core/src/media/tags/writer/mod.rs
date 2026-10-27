mod flac;
mod mp3;
mod mp4;

use super::{StandardTags, TagPlan, encode_comment};
use crate::{
    Error, Result,
    media::{open_regular, validate_cover},
};
use lofty::{
    config::{ParseOptions, WriteOptions},
    file::FileType,
    picture::{MimeType, Picture, PictureType},
    probe::Probe,
};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, Seek, Write},
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TagContainer {
    Mp3,
    Flac,
    Mp4,
}
#[derive(Debug, Serialize)]
pub struct TagWriteReport {
    pub container: TagContainer,
    pub cover: bool,
    pub tags: StandardTags,
    pub bytes: u64,
}

/// Produce a new media file without changing or overwriting the source or any
/// existing output. Writes through a same-directory temporary, then syncs and
/// publishes with no-clobber semantics. Blocking: run on a worker thread.
pub fn write_tagged_copy(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    plan: &TagPlan,
    cover: Option<&[u8]>,
) -> Result<TagWriteReport> {
    validate_plan(plan)?;
    let picture = cover.map(cover_picture).transpose()?;
    let output = output.as_ref();
    match std::fs::symlink_metadata(output) {
        Ok(_) => {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "tag output exists").into());
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let mut source = open_regular(source)?;
    let before = source.metadata()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staging = tempfile::Builder::new()
        .prefix(".cumulus-tags-")
        .tempfile_in(parent)?;
    let copied = io::copy(&mut source, &mut staging)?;
    let after = source.metadata()?;
    if copied != before.len()
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(Error::State("tag source changed during copy"));
    }
    let report = write_staging(staging.as_file_mut(), plan, picture)?;
    staging.flush()?;
    staging.as_file().sync_all()?;
    staging
        .persist_noclobber(output)
        .map_err(|e| Error::Io(e.error))?;
    Ok(report)
}

fn validate_plan(plan: &TagPlan) -> Result<()> {
    if plan.encrypted {
        return Err(Error::InvalidInput("encrypted plans require NCM output"));
    }
    if [
        &plan.tags.tit2,
        &plan.tags.tpe1,
        &plan.tags.talb,
        &plan.tags.trck,
        &plan.tags.tpos,
    ]
    .into_iter()
    .any(|s| s.len() > 1024 * 1024 || s.contains('\0'))
    {
        return Err(Error::InvalidInput(
            "tag text exceeds limit or contains NUL",
        ));
    }
    // Validation happens before creating a staging file.
    encode_comment(&plan.comment_plain)?;
    Ok(())
}

fn cover_picture(data: &[u8]) -> Result<Picture> {
    let info = validate_cover(data)?;
    Ok(Picture::unchecked(data.to_vec())
        .mime_type(MimeType::from_str(info.mime))
        .pic_type(PictureType::CoverFront)
        .description("Cover")
        .build())
}

fn write_staging(
    file: &mut File,
    plan: &TagPlan,
    picture: Option<Picture>,
) -> Result<TagWriteReport> {
    file.rewind()?;
    let kind = Probe::new(&mut *file).guess_file_type()?.file_type();
    file.rewind()?;
    let comment = encode_comment(&plan.comment_plain)?;
    let has_cover = picture.is_some();
    let container = match kind {
        Some(FileType::Mpeg) => {
            mp3::write(file, &plan.tags, &comment, picture)?;
            TagContainer::Mp3
        }
        Some(FileType::Flac) => {
            flac::write(file, &plan.tags, &comment, picture)?;
            TagContainer::Flac
        }
        Some(FileType::Mp4) => {
            mp4::write(file, &plan.tags, &comment, picture)?;
            TagContainer::Mp4
        }
        _ => return Err(Error::Media("tag writing supports MP3, FLAC and MP4/M4A")),
    };
    Ok(TagWriteReport {
        container,
        cover: has_cover,
        tags: plan.tags.clone(),
        bytes: file.metadata()?.len(),
    })
}

/// Caller owns and journals the staging file; no destination is published.
pub(crate) fn prepare_file(
    source: &Path,
    staging: &mut File,
    plan: &TagPlan,
    cover: Option<&[u8]>,
) -> Result<TagWriteReport> {
    validate_plan(plan)?;
    let picture = cover.map(cover_picture).transpose()?;
    let mut source = open_regular(source)?;
    staging.set_len(0)?;
    staging.rewind()?;
    io::copy(&mut source, staging)?;
    write_staging(staging, plan, picture)
}
fn parse_options() -> ParseOptions {
    ParseOptions::new()
        .read_properties(false)
        .implicit_conversions(false)
}
fn write_options() -> WriteOptions {
    WriteOptions::default()
}
