use super::{EventKind, EventSample};
use crate::{Error, Result};
use serde_json::{Value, json};

pub(super) fn numeric(value: f64) -> Value {
    json!(if value.is_finite() {
        value.trunc().rem_euclid(4294967296.) as u32
    } else {
        0
    })
}
pub(super) fn flag(value: Option<bool>) -> Value {
    json!(match value {
        None => 1,
        Some(true) => 2,
        Some(false) => 3,
    })
}
pub(super) fn round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.
    } else {
        floor
    }
}
pub(super) fn relative(now: u64, reference: u64) -> Value {
    numeric((i128::from(now) - i128::from(reference)) as f64)
}
fn nonzero(value: Option<f64>) -> Option<f64> {
    value.filter(|v| *v != 0. && !v.is_nan())
}

pub(super) fn values(
    kind: EventKind,
    event: &EventSample,
    count: u16,
    now: u64,
    reference: u64,
) -> Result<Option<Value>> {
    let point = event.touch.as_ref().unwrap_or(&event.point);
    // Preserve the recovered clientX fallback for Y; it is not screenY.
    let x = nonzero(point.client_x)
        .or(point.screen_x)
        .unwrap_or(f64::NAN);
    let y = nonzero(point.client_y)
        .or(point.client_x)
        .unwrap_or(f64::NAN);
    let mut out = vec![json!(count), relative(now, reference)];
    match kind {
        EventKind::Down => out.extend([
            flag(event.trusted),
            numeric(event.button.unwrap_or(0.)),
            numeric(x),
            numeric(y),
        ]),
        EventKind::Move | EventKind::Up => {
            out.extend([flag(event.trusted), numeric(x), numeric(y)])
        }
        EventKind::Click => {
            if !event.target_present {
                return Err(Error::InvalidInput("Watchman click has no target"));
            }
            out.extend([
                flag(event.trusted),
                numeric(x),
                numeric(y),
                json!(event.target_id),
            ]);
        }
        EventKind::Keydown | EventKind::Focus | EventKind::Blur => out.extend([
            flag(event.trusted),
            json!(if event.target_present {
                event.target_id.as_str()
            } else {
                ""
            }),
        ]),
        EventKind::Scroll => {
            let p = event.scroll.unwrap_or([f64::NAN; 2]);
            out.extend([flag(event.trusted), numeric(p[0]), numeric(p[1])]);
        }
        EventKind::Orientation => {
            let Some(p) = &event.orientation else {
                return Ok(None);
            };
            let (Some(a), Some(b), Some(c)) = (p.alpha, p.beta, p.gamma) else {
                return Ok(None);
            };
            out.extend([
                numeric(round(a)),
                numeric(round(b)),
                numeric(round(c)),
                flag(p.absolute),
            ]);
        }
        EventKind::Motion => {
            let Some(p) = &event.motion else {
                return Ok(None);
            };
            let Some([Some(a), Some(b), Some(c)]) = p.acceleration.or(p.including_gravity) else {
                return Ok(None);
            };
            out.extend([
                numeric(round(a * 1000.)),
                numeric(round(b * 1000.)),
                numeric(round(c * 1000.)),
            ]);
            // Undefined/null interval is skipped by the SDK ARRAY encoder.
            if let Some(interval) = p.interval {
                out.push(numeric(interval));
            }
        }
        EventKind::Battery => return Err(Error::InvalidInput("use battery observation entry")),
    }
    Ok(Some(Value::Array(out)))
}
