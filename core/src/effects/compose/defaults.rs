//! M/28 N: official built-in fallback parameters, separate from entitlement.
use super::{Composition, compose};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Enhancement {
    #[serde(rename = "en_bass")]
    Bass,
    #[serde(rename = "en_treble")]
    Treble,
    #[serde(rename = "en_vocal")]
    Vocal,
    #[serde(rename = "en_vibe")]
    Vibe,
    #[serde(rename = "en_live")]
    Live,
}
impl Enhancement {
    pub fn key(self) -> &'static str {
        match self {
            Self::Bass => "en_bass",
            Self::Treble => "en_treble",
            Self::Vocal => "en_vocal",
            Self::Vibe => "en_vibe",
            Self::Live => "en_live",
        }
    }
}
fn parameters() -> &'static Value {
    static VALUE: OnceLock<Value> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!("defaults.json"))
            .expect("checked built-in effect parameters")
    })
}
pub fn builtin_base() -> Value {
    parameters()["aimaster-base-ae"].clone()
}
pub fn builtin_diy(selected: &[Enhancement]) -> Value {
    let mut result = Map::new();
    for option in selected {
        result.insert(
            option.key().to_owned(),
            parameters()["aimaster-diy-ae"][option.key()].clone(),
        );
    }
    Value::Object(result)
}
/// Fallback material composition only. The host must separately check rights
/// and ensure the selected DSP graph is implemented before activating sound.
pub fn compose_builtin(
    song: Option<&Value>,
    selected: &[Enhancement],
    support_stereo: bool,
) -> Result<Composition> {
    let mut input = json!({"baseJson":builtin_base(),"diyJson":builtin_diy(selected),"supportStereo":support_stereo});
    if let Some(song) = song {
        input["songJson"] = song.clone();
    }
    compose(&input)
}
