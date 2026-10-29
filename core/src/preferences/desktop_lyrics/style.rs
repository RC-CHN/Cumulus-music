//! Recovered M81 desktop-lyric style tables. Hex values retain native AARRGGBB
//! when eight digits are present; they are never interpreted as CSS RRGGBBAA.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
/// M81's ordered native fallback candidates. The last Songti fallback is the
/// recovered final choice; hosts append their platform generic fallback.
pub const FONT_FALLBACKS: [&str; 6] = ["微软雅黑", "幼圆", "楷体", "隶书", "新宋体", "宋体"];
pub fn font_families(requested: &str) -> Vec<String> {
    let mut names = Vec::new();
    if !requested.is_empty() {
        names.push(requested.to_owned());
    }
    for name in FONT_FALLBACKS {
        if !names.iter().any(|v| v == name) {
            names.push(name.to_owned());
        }
    }
    names
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Auxiliary {
    None,
    #[default]
    Translate,
    Roman,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Composition {
    #[default]
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Center,
    Split,
    Left,
    Right,
}
impl Alignment {
    /// Exact M81 L.one/L.two ordering; split has no effect in single-line mode.
    pub fn pair(self, single: bool) -> [&'static str; 2] {
        match (single, self) {
            (_, Self::Center) | (true, Self::Split) => ["center", "center"],
            (false, Self::Split) => ["left", "right"],
            (true, Self::Left) => ["left", "center"],
            (true, Self::Right) => ["right", "center"],
            (false, Self::Left) => ["left", "left"],
            (false, Self::Right) => ["right", "right"],
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Gray,
    Sunset,
    Pink,
    Purple,
    Blue,
    Green,
    Golden,
    #[default]
    Red,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    /// Unplayed gradient top/bottom, then played gradient top/bottom.
    pub text: [String; 4],
    /// Unplayed, played outline colors.
    pub outline: [String; 2],
}
impl Preset {
    pub fn colors(self) -> Colors {
        let (played, outline) = match self {
            Self::Gray => (["ebe9e9", "bfbfbf"], "8b8b8b"),
            Self::Sunset => (["ffbf99", "e0c3ff"], "8e76c2"),
            Self::Pink => (["ffeef7", "fcbede"], "be80a0"),
            Self::Purple => (["f2efff", "ded8f8"], "998dc9"),
            Self::Blue => (["e4f6ff", "acd3e5"], "5495b4"),
            Self::Green => (["f1ffe0", "dcf6c3"], "67905c"),
            Self::Golden => (["fff8ea", "fadda7"], "ff9f8953"),
            Self::Red => (["ffdede", "ffb0b0"], "cb7474"),
        };
        Colors {
            text: ["eeeeee", "eeeeee", played[0], played[1]].map(str::to_owned),
            outline: ["999999", outline].map(str::to_owned),
        }
    }
}
/// Normalize a native RGB/ARGB value to an unambiguous channel tuple.
pub fn rgba(hex: &str) -> Result<[u8; 4]> {
    if ![6, 8].contains(&hex.len()) || !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput("invalid desktop lyric color"));
    }
    let number = u32::from_str_radix(hex, 16)
        .map_err(|_| Error::InvalidInput("invalid desktop lyric color"))?;
    Ok([
        (number >> 16) as u8,
        (number >> 8) as u8,
        number as u8,
        if hex.len() == 8 {
            (number >> 24) as u8
        } else {
            255
        },
    ])
}
impl Colors {
    pub fn validate(&self) -> Result<()> {
        for value in self.text.iter().chain(self.outline.iter()) {
            rgba(value)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Palette {
    Preset { preset: Preset },
    Custom { colors: Colors },
}
impl Default for Palette {
    fn default() -> Self {
        Self::Preset {
            preset: Preset::Red,
        }
    }
}
impl Palette {
    pub fn colors(&self) -> Colors {
        match self {
            Self::Preset { preset } => preset.colors(),
            Self::Custom { colors } => colors.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    pub auxiliary: Auxiliary,
    pub always_on_top: bool,
    pub font: String,
    pub font_size: u16,
    pub bold: bool,
    pub outline: bool,
    pub single_line: bool,
    pub composition: Composition,
    pub single_alignment: Alignment,
    pub double_alignment: Alignment,
    pub palette: Palette,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            auxiliary: Auxiliary::Translate,
            always_on_top: true,
            font: String::new(),
            font_size: 36,
            bold: false,
            outline: true,
            single_line: true,
            composition: Composition::Horizontal,
            single_alignment: Alignment::Center,
            double_alignment: Alignment::Center,
            palette: Palette::default(),
        }
    }
}
impl Appearance {
    pub fn validate(&self) -> Result<()> {
        if !(12..=96).contains(&self.font_size)
            || self.font.len() > 256
            || self.font.chars().any(char::is_control)
        {
            return Err(Error::InvalidInput("invalid desktop lyric font"));
        }
        self.palette.colors().validate()
    }
    pub fn alignments(&self) -> [&'static str; 2] {
        if self.single_line {
            self.single_alignment
        } else {
            self.double_alignment
        }
        .pair(self.single_line)
    }
}
