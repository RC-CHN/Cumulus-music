use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioMetadata {
    pub format: Option<String>,
    /// Seconds, finite and nonnegative. CUE requires a positive source duration.
    pub duration: f64,
    pub title: String,
    pub album: String,
    pub artists: Vec<String>,
    pub bitrate: Option<u64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub container: String,
    pub cover_bytes: Option<u64>,
}
impl AudioMetadata {
    /// A WAV file may contain pcm_s16le; the file-format column names its
    /// container. NCM instead displays the sniffed inner audio format.
    pub fn file_format(&self) -> &str {
        if self.container == "ncm" {
            self.format.as_deref().unwrap_or("ncm")
        } else {
            &self.container
        }
    }
}
