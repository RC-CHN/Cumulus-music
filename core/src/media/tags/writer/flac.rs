use super::{parse_options, write_options};
use crate::{Error, Result, media::tags::StandardTags};
use lofty::{
    file::AudioFile,
    flac::FlacFile,
    ogg::OggPictureStorage,
    picture::{Picture, PictureType},
    tag::TagExt,
};
use std::{fs::File, io::Seek};

pub(super) fn write(
    file: &mut File,
    tags: &StandardTags,
    comment: &str,
    picture: Option<Picture>,
) -> Result<()> {
    let parsed = FlacFile::read_from(file, parse_options())
        .map_err(|_| Error::Media("cannot parse FLAC tags"))?;
    let mut tag = parsed.vorbis_comments().cloned().unwrap_or_default();
    for (pic, info) in parsed.pictures() {
        tag.insert_picture(pic.clone(), Some(*info))
            .map_err(|_| Error::Media("invalid existing FLAC picture"))?;
    }
    for (key, text) in [
        ("TITLE", &tags.tit2),
        ("ARTIST", &tags.tpe1),
        ("ALBUM", &tags.talb),
        ("TRACKNUMBER", &tags.trck),
        ("DISCNUMBER", &tags.tpos),
    ] {
        if !text.is_empty() {
            tag.insert(key.into(), text.clone());
        }
    }
    tag.insert("DESCRIPTION".into(), comment.into());
    if let Some(picture) = picture {
        tag.remove_picture_type(PictureType::CoverFront);
        tag.insert_picture(picture, None)
            .map_err(|_| Error::Media("invalid FLAC picture"))?;
    }
    file.rewind()?;
    // Write only Vorbis/PICTURE blocks; don't rewrite an unrelated legacy ID3.
    tag.save_to(file, write_options())
        .map_err(|_| Error::Media("cannot write FLAC tags"))
}
