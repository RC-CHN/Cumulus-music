use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Serialize)]
pub struct TimedWord {
    pub time: f64,
    pub duration: f64,
    pub text: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct WordLine {
    pub time: f64,
    pub duration: f64,
    pub text: String,
    pub words: Vec<TimedWord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    pub lyric: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LyricSource {
    None,
    Lrc,
    Yrc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayType {
    Default,
    Pure,
    Ask,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YrcInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yrc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yrc_roma: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yrc_trans: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLyrics {
    pub current_used_lyric: LyricSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_used_lyric_version: Option<Value>,
    pub display_type: DisplayType,
    pub lyric_lines: Vec<LyricLine>,
    pub roma_lyric_lines: Vec<LyricLine>,
    pub tlyric_lines: Vec<LyricLine>,
    pub offset: f64,
    pub scrollable: bool,
    pub yrc_info: YrcInfo,
    /// Only the six documented contributor/permission fields are copied here.
    #[serde(flatten)]
    pub metadata: Map<String, Value>,
}

impl Default for ParsedLyrics {
    fn default() -> Self {
        Self {
            current_used_lyric: LyricSource::None,
            current_used_lyric_version: Some(Value::from(-1)),
            display_type: DisplayType::Default,
            lyric_lines: Vec::new(),
            roma_lyric_lines: Vec::new(),
            tlyric_lines: Vec::new(),
            offset: 0.0,
            scrollable: false,
            yrc_info: YrcInfo::default(),
            metadata: Map::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedLrc {
    pub head_artist_infos: Vec<Value>,
    pub tail_artist_infos: Vec<Value>,
    pub lyric_lines: Vec<LyricLine>,
    pub scrollable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedYrc {
    pub head_artist_infos: Vec<Value>,
    pub tail_artist_infos: Vec<Value>,
    pub lyric_lines: Vec<LyricLine>,
    pub raw_yrc: String,
}
