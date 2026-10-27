use super::{parse_options, write_options};
use crate::{Error, Result, media::tags::StandardTags};
use lofty::{
    file::AudioFile,
    mp4::{Atom, AtomData, AtomIdent, Mp4File},
    picture::Picture,
    tag::{Accessor, TagExt},
};
use std::{fs::File, io::Seek};

pub(super) fn write(
    file: &mut File,
    tags: &StandardTags,
    comment: &str,
    picture: Option<Picture>,
) -> Result<()> {
    let track = pair(&tags.trck)?;
    let disk = pair(&tags.tpos)?;
    let parsed = Mp4File::read_from(file, parse_options())
        .map_err(|_| Error::Media("cannot parse MP4 tags"))?;
    let mut tag = parsed.ilst().cloned().unwrap_or_default();
    for (key, text) in [
        (*b"\xa9nam", &tags.tit2),
        (*b"\xa9ART", &tags.tpe1),
        (*b"\xa9alb", &tags.talb),
    ] {
        if !text.is_empty() {
            tag.replace_atom(Atom::new(
                AtomIdent::Fourcc(key),
                AtomData::UTF8(text.clone()),
            ));
        }
    }
    if let Some((number, total)) = track {
        tag.set_track(number);
        tag.set_track_total(total);
    }
    if let Some((number, total)) = disk {
        tag.set_disk(number);
        tag.set_disk_total(total);
    }
    tag.replace_atom(Atom::new(
        AtomIdent::Fourcc(*b"\xa9cmt"),
        AtomData::UTF8(comment.into()),
    ));
    if let Some(picture) = picture {
        tag.remove_pictures();
        tag.insert_picture(picture);
    }
    file.rewind()?;
    tag.save_to(file, write_options())
        .map_err(|_| Error::Media("cannot write MP4 tags"))
}
fn pair(text: &str) -> Result<Option<(u32, u32)>> {
    let pieces = text.split('/').collect::<Vec<_>>();
    if !(1..=2).contains(&pieces.len())
        || pieces
            .iter()
            .any(|s| s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()))
    {
        return Ok(None);
    }
    let parse = |s: &str| {
        s.parse::<u16>()
            .map(u32::from)
            .map_err(|_| Error::InvalidInput("MP4 track/disc number exceeds 65535"))
    };
    Ok(Some((
        parse(pieces[0])?,
        if pieces.len() == 2 {
            parse(pieces[1])?
        } else {
            0
        },
    )))
}
