//! Text lyric tags in ordinary local audio, read without decoding audio.
use lofty::{
    config::ParseOptions,
    file::{FileType, TaggedFileExt},
    probe::Probe,
    tag::ItemKey,
};
use std::path::Path;

pub(super) fn read(path: &Path, limit: usize) -> Option<String> {
    let file = crate::media::open_regular(path).ok()?;
    let probe = Probe::new(file).guess_file_type().ok()?;
    let keys = match probe.file_type()? {
        FileType::Flac | FileType::Mp4 => [ItemKey::Lyrics, ItemKey::UnsyncLyrics],
        FileType::Mpeg => [ItemKey::UnsyncLyrics, ItemKey::Lyrics],
        _ => return None,
    };
    let tagged = probe
        .options(
            ParseOptions::new()
                .read_properties(false)
                .read_cover_art(false),
        )
        .read()
        .ok()?;
    keys.iter()
        .flat_map(|key| tagged.tags().iter().filter_map(|tag| tag.get_string(*key)))
        .find(|text| text.len() <= limit && !text.trim().is_empty())
        .map(str::to_owned)
}
