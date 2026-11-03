//! cloudmusic EFB760 selects the modern object as a whole, then dispatches
//! components in a fixed order between the engine's begin/end update calls.
//! This is a native parameter plan, not an assertion that its DSP is available.
use crate::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MaterialSchema {
    Legacy,
    Version2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    Eq,
    Bt,
    Se,
    Rvb,
    Rotate,
    Peq,
    Cmp,
    Limiter,
    Dly,
    Alc,
    Spspatial,
}
impl Component {
    pub fn key(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Bt => "bt",
            Self::Se => "se",
            Self::Rvb => "rvb",
            Self::Rotate => "rotate",
            Self::Peq => "peq",
            Self::Cmp => "cmp",
            Self::Limiter => "limiter",
            Self::Dly => "dly",
            Self::Alc => "alc",
            Self::Spspatial => "spspatial",
        }
    }
}
const COMPONENTS: [Component; 11] = [
    Component::Eq,
    Component::Bt,
    Component::Se,
    Component::Rvb,
    Component::Rotate,
    Component::Peq,
    Component::Cmp,
    Component::Limiter,
    Component::Dly,
    Component::Alc,
    Component::Spspatial,
];

#[derive(Debug, Serialize)]
pub struct ComponentSection<'a> {
    pub component: Component,
    pub section: &'a Value,
}

#[derive(Debug)]
pub struct MaterialRouting<'a> {
    pub schema: MaterialSchema,
    /// Includes off/missing-on component objects: their calls can disable an
    /// existing node. Do not filter these by truthiness or reuse old parameters.
    pub sections: Vec<ComponentSection<'a>>,
}

/// Only an object-valued aev2 selects the modern branch. An empty object still
/// suppresses every legacy component. Mistyped component values are ignored.
/// Legacy reads five components; top-level PEQ/limiter/etc are not dispatched.
/// The official modern chorus/flanger branches log and make no engine calls.
pub fn route_sections(material: &Value) -> Result<MaterialRouting<'_>> {
    let root = super::object(material)?;
    let (schema, source, count) = match root.get("aev2").and_then(Value::as_object) {
        Some(modern) => (MaterialSchema::Version2, modern, COMPONENTS.len()),
        None => (MaterialSchema::Legacy, root, 5),
    };
    let sections = COMPONENTS[..count]
        .iter()
        .filter_map(|&component| {
            source
                .get(component.key())
                .filter(|value| value.is_object())
                .map(|section| ComponentSection { component, section })
        })
        .collect();
    Ok(MaterialRouting { schema, sections })
}
