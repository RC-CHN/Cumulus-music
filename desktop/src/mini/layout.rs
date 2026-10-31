use cumulus_core::{Error, Result};
use serde::Deserialize;
use tauri::PhysicalSize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    width: u32,
    height: u32,
    scale: f64,
}
impl Viewport {
    pub fn fits(&self) -> bool {
        self.width.abs_diff(420) <= 1 && self.height.abs_diff(206) <= 1
    }
    pub fn size(&self) -> Result<PhysicalSize<u32>> {
        size(self.scale, false)
    }
}
pub fn size(scale: f64, expanded: bool) -> Result<PhysicalSize<u32>> {
    if !scale.is_finite() || !(0.5..=8.0).contains(&scale) {
        return Err(Error::InvalidInput("invalid mini display scale"));
    }
    Ok(PhysicalSize::new(
        (420.0 * scale).round() as u32,
        (if expanded { 516.0 } else { 206.0 } * scale).round() as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn webview_scale_determines_content_pixels_independently_of_native_scale() {
        assert_eq!(size(1.25, false).unwrap(), PhysicalSize::new(525, 258));
        assert_eq!(size(1.25, true).unwrap(), PhysicalSize::new(525, 645));
        assert_eq!(size(2.0, false).unwrap(), PhysicalSize::new(840, 412));
        assert!(
            !Viewport {
                width: 336,
                height: 164,
                scale: 1.25
            }
            .fits()
        );
        assert!(
            Viewport {
                width: 420,
                height: 206,
                scale: 1.25
            }
            .fits()
        );
        for scale in [f64::NAN, f64::INFINITY, -1.0, 0.0, 9.0] {
            assert!(size(scale, false).is_err());
        }
    }
}
