use super::{parse_options, write_options};
use crate::{Error, Result, media::tags::StandardTags};
use lofty::{
    TextEncoding,
    file::AudioFile,
    id3::v2::{CommentFrame, FrameId, TextInformationFrame},
    mpeg::MpegFile,
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
    let parsed = MpegFile::read_from(file, parse_options())
        .map_err(|_| Error::Media("cannot parse MP3 tags"))?;
    let mut tag = parsed.id3v2().cloned().unwrap_or_default();
    for (key, text) in [
        ("TIT2", &tags.tit2),
        ("TPE1", &tags.tpe1),
        ("TALB", &tags.talb),
        ("TRCK", &tags.trck),
        ("TPOS", &tags.tpos),
    ] {
        if !text.is_empty() {
            tag.insert(
                TextInformationFrame::new(
                    FrameId::new(key).expect("static frame"),
                    TextEncoding::UTF8,
                    text.clone(),
                )
                .into(),
            );
        }
    }
    // Only replace COMM with this language and empty description; leave other
    // user comments, USLT, private frames and non-front artwork in place.
    tag.insert(CommentFrame::new(TextEncoding::UTF8, *b"eng", "", comment.to_owned()).into());
    if let Some(picture) = picture {
        tag.remove_picture_type(PictureType::CoverFront);
        tag.insert_picture(picture);
    }
    file.rewind()?;
    tag.save_to(file, write_options())
        .map_err(|_| Error::Media("cannot write MP3 tags"))
}
