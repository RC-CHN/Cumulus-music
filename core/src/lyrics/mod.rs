//! Official M/370 lyric parsing and presentation, with finite playback clocks.
//! Network fetching stays in CatalogApi; this domain also accepts local lyrics.
mod credits;
pub mod files;
mod format;
mod lrc;
mod presentation;
mod response;
mod service;
mod types;
mod yrc;

pub use format::{format_timestamp, protect_timestamp_only_lines, render_lrc};
pub use lrc::parse_lrc;
pub use presentation::{active_line, parse_lyrics};
pub use response::{normalize_cloud_response, normalize_song_response};
pub use service::{
    LyricDemand, LyricPreferences, LyricRequest, LyricSnapshot, LyricTarget, LyricsService,
};
pub use types::{DisplayType, LyricLine, LyricSource, ParsedLrc, ParsedLyrics, ParsedYrc, YrcInfo};
pub use types::{TimedWord, WordLine};
pub use yrc::parse_yrc;
pub use yrc::parse_yrc_words;

pub mod desktop;
