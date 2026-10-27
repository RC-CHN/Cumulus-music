use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Move,
    Down,
    Up,
    Click,
    Keydown,
    Focus,
    Blur,
    Scroll,
    Orientation,
    Motion,
    Battery,
}
impl EventKind {
    pub fn from_dom(name: &str) -> Option<Self> {
        Some(match name {
            "mousemove" | "touchmove" | "pointermove" | "mspointermove" => Self::Move,
            "mousedown" | "touchstart" | "pointerdown" | "mspointerdown" => Self::Down,
            "mouseup" | "touchend" | "pointerup" | "mspointerup" => Self::Up,
            "click" => Self::Click,
            "keydown" => Self::Keydown,
            "focus" => Self::Focus,
            "blur" => Self::Blur,
            "scroll" => Self::Scroll,
            "deviceorientation" => Self::Orientation,
            "devicemotion" => Self::Motion,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Move => "_move",
            Self::Down => "_down",
            Self::Up => "_up",
            Self::Click => "_click",
            Self::Keydown => "_keydown",
            Self::Focus => "_focus",
            Self::Blur => "_blur",
            Self::Scroll => "_scroll",
            Self::Orientation => "_orientation",
            Self::Motion => "_motion",
            Self::Battery => "_battery",
        }
    }
}
/// Browser measurements only: deliberately no key code, input value, text,
/// password or authorization data. No Debug implementation for observations.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventSample {
    pub dom_type: String,
    pub trusted: Option<bool>,
    #[serde(default)]
    pub target_id: String,
    #[serde(default = "present")]
    pub target_present: bool,
    #[serde(default)]
    pub point: PointerPoint,
    pub touch: Option<PointerPoint>,
    pub button: Option<f64>,
    pub scroll: Option<[f64; 2]>,
    pub orientation: Option<Orientation>,
    pub motion: Option<Motion>,
}
impl Default for EventSample {
    fn default() -> Self {
        Self {
            dom_type: String::new(),
            trusted: None,
            target_id: String::new(),
            target_present: true,
            point: PointerPoint::default(),
            touch: None,
            button: None,
            scroll: None,
            orientation: None,
            motion: None,
        }
    }
}
fn present() -> bool {
    true
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PointerPoint {
    pub client_x: Option<f64>,
    pub client_y: Option<f64>,
    pub screen_x: Option<f64>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Orientation {
    pub alpha: Option<f64>,
    pub beta: Option<f64>,
    pub gamma: Option<f64>,
    pub absolute: Option<bool>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    pub acceleration: Option<[Option<f64>; 3]>,
    pub including_gravity: Option<[Option<f64>; 3]>,
    pub interval: Option<f64>,
}
/// null/absent battery times denote the browser's non-finite/unknown time;
/// the wire encodes -1. Partial change events fall back to the initial reading.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatterySample {
    pub charging: Option<bool>,
    pub level: Option<f64>,
    pub charging_time: Option<f64>,
    pub discharging_time: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleOutcome {
    Stored,
    Throttled,
    MissingMeasurement,
    Stopped,
}
