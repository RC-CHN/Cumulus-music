//! CUE INDEX 01 segments, matching the previously verified subset. This does
//! not implement mixed data discs or synthesize INDEX 00/PREGAP silence.
mod text;
mod tokens;
use super::{AudioMetadata, probe::AudioProbe, supported_audio};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CueTrack {
    pub path: PathBuf,
    pub cue: PathBuf,
    pub track: u8,
    pub title: String,
    pub album: String,
    pub artist: String,
    pub offset: f64,
    pub duration: f64,
    pub source_duration: f64,
}

pub struct CueSheet {
    pub tracks: Vec<CueTrack>,
    pub sources: BTreeMap<PathBuf, AudioMetadata>,
}
struct Pending {
    number: u8,
    file: Option<PathBuf>,
    title: String,
    artist: String,
    offset: Option<f64>,
}

pub fn parse_cue(path: impl AsRef<Path>, probe: &dyn AudioProbe) -> Result<CueSheet> {
    let path = path.as_ref().canonicalize()?;
    let file = crate::media::open_regular(&path)?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(Error::Media("CUE text exceeds 4MiB limit"));
    }
    let text = text::decode(&bytes)?;
    let mut file = None;
    let mut current: Option<usize> = None;
    let mut tracks: Vec<Pending> = Vec::new();
    let mut numbers = HashSet::new();
    let mut album = String::new();
    let mut performer = String::new();
    for line in text.split(['\r', '\n']) {
        let parts = tokens::split(line)?;
        if parts.is_empty() {
            continue;
        }
        match parts[0].to_ascii_uppercase().as_str() {
            "FILE" if parts.len() >= 2 => {
                let source = path
                    .parent()
                    .expect("canonical file parent")
                    .join(parts[1].replace('\\', "/"))
                    .canonicalize()?;
                if !source.is_file() || !supported_audio(&source) {
                    return Err(Error::Media("CUE source is not supported local audio"));
                }
                file = Some(source);
                current = None;
            }
            "TRACK" if parts.len() >= 3 => {
                if !parts[2].eq_ignore_ascii_case("AUDIO") {
                    return Err(Error::Media("mixed data/audio CUE is unsupported"));
                }
                let number = parts[1]
                    .parse::<u8>()
                    .ok()
                    .filter(|v| (1..=99).contains(v))
                    .ok_or(Error::Media("invalid CUE track number"))?;
                if !numbers.insert(number) {
                    return Err(Error::Media("duplicate CUE track number"));
                }
                tracks.push(Pending {
                    number,
                    file: file.clone(),
                    title: String::new(),
                    artist: performer.clone(),
                    offset: None,
                });
                current = Some(tracks.len() - 1);
            }
            "TITLE" | "PERFORMER" if parts.len() >= 2 => {
                let value = parts[1..].join(" ");
                let title = parts[0].eq_ignore_ascii_case("TITLE");
                if let Some(i) = current {
                    if title {
                        tracks[i].title = value;
                    } else {
                        tracks[i].artist = value;
                    }
                } else if title {
                    album = value;
                } else {
                    performer = value;
                }
            }
            "INDEX" if parts.len() == 3 && parts[1] == "01" => {
                if let Some(i) = current {
                    if tracks[i].offset.is_some() {
                        return Err(Error::Media("duplicate CUE INDEX 01"));
                    }
                    tracks[i].offset = Some(timestamp(&parts[2])?);
                }
            }
            _ => (),
        }
    }
    if tracks.is_empty() {
        return Err(Error::Media("CUE has no audio tracks"));
    }
    let mut sources = BTreeMap::new();
    let mut result = Vec::new();
    for (i, track) in tracks.iter().enumerate() {
        let file = track
            .file
            .as_ref()
            .ok_or(Error::Media("CUE track has no FILE"))?;
        let offset = track
            .offset
            .ok_or(Error::Media("CUE track has no INDEX 01"))?;
        if !sources.contains_key(file) {
            let metadata = probe.inspect(file)?;
            if !metadata.duration.is_finite() || metadata.duration <= 0.0 {
                return Err(Error::Media("CUE source has no finite positive duration"));
            }
            sources.insert(file.clone(), metadata);
        }
        let duration = sources[file].duration;
        let end = match tracks[i + 1..]
            .iter()
            .find(|t| t.file.as_ref() == Some(file))
        {
            Some(t) => t.offset.ok_or(Error::Media("CUE track has no INDEX 01"))?,
            None => duration,
        };
        if offset >= end || end > duration + 0.001 {
            return Err(Error::Media("invalid CUE segment boundaries"));
        }
        result.push(CueTrack {
            path: file.clone(),
            cue: path.clone(),
            track: track.number,
            title: track.title.clone(),
            album: album.clone(),
            artist: track.artist.clone(),
            offset,
            duration: end - offset,
            source_duration: duration,
        });
    }
    Ok(CueSheet {
        tracks: result,
        sources,
    })
}

fn timestamp(text: &str) -> Result<f64> {
    let parts = text
        .split(':')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::Media("invalid CUE timestamp"))?;
    if parts.len() != 3 || parts[1] >= 60 || parts[2] >= 75 {
        return Err(Error::Media("invalid CUE timestamp"));
    }
    let value = parts[0] as f64 * 60.0 + parts[1] as f64 + parts[2] as f64 / 75.0;
    if !value.is_finite() {
        return Err(Error::Media("CUE timestamp overflow"));
    }
    Ok(value)
}
