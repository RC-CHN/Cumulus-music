//! Installation-local desktop lyric intent. Windows remain the desktop host's
//! responsibility; damaged state never opens a window or enables click-through.
mod storage;
mod style;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
pub use style::{Alignment, Appearance, Auxiliary, Colors, Composition, Palette, Preset, rgba};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Geometry {
    pub fn validate(&self) -> Result<()> {
        if !(-1_000_000..=1_000_000).contains(&self.x)
            || !(-1_000_000..=1_000_000).contains(&self.y)
            || !(240..=4096).contains(&self.width)
            || !(80..=4096).contains(&self.height)
        {
            return Err(Error::InvalidInput("invalid desktop lyric geometry"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopLyricPreferences {
    pub revision: String,
    pub enabled: bool,
    pub appearance: Appearance,
    pub geometry: Option<Geometry>,
    pub custom_palette: Option<Colors>,
}
impl Default for DesktopLyricPreferences {
    fn default() -> Self {
        Self {
            revision: "initial".into(),
            enabled: false,
            appearance: Appearance::default(),
            geometry: None,
            custom_palette: None,
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Enabled { value: bool },
    Appearance { value: Appearance },
    Geometry { value: Option<Geometry> },
    CustomColors { colors: Colors },
    Reset,
}
impl DesktopLyricPreferences {
    fn validate(&self) -> Result<()> {
        self.appearance.validate()?;
        if let Some(geometry) = &self.geometry {
            geometry.validate()?;
        }
        if let Some(colors) = &self.custom_palette {
            colors.validate()?;
        }
        Ok(())
    }
    fn apply(&mut self, edit: Edit) -> Result<()> {
        match edit {
            Edit::Enabled { value } => self.enabled = value,
            Edit::Appearance { value } => {
                if let Palette::Custom { colors } = &value.palette {
                    self.custom_palette = Some(colors.clone());
                }
                self.appearance = value;
            }
            Edit::Geometry { value } => self.geometry = value,
            Edit::CustomColors { colors } => {
                self.appearance.palette = Palette::Custom {
                    colors: colors.clone(),
                };
                self.custom_palette = Some(colors);
            }
            Edit::Reset => *self = Self::default(),
        }
        self.validate()
    }
}
