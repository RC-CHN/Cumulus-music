//! Deterministic Windows hardware profile for the optional compatibility layer.
//!
//! Nothing here observes the host. A per-install seed selects one plausible
//! Windows desktop configuration, so every launch of the same installation
//! reports identical values while different installations stay distinct. This
//! is a chosen identity: not a measurement, and not recovered client behaviour.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Desktop configurations bracketed by measured Windows references: a 4-core
/// CI runner at 1024x768 and a 32-core desktop at 2048x1152 CSS pixels. Physical
/// size and scale factor stay separate because the web platform reports CSS
/// pixels plus a device pixel ratio.
const DESKTOPS: [((u32, u32), f64, u32); 6] = [
    ((1366, 768), 1.0, 4),
    ((1920, 1080), 1.0, 8),
    ((1920, 1080), 1.25, 12),
    ((2560, 1440), 1.0, 16),
    ((2560, 1440), 1.25, 16),
    ((3840, 2160), 1.5, 16),
];

/// Height the Windows taskbar reserves, in CSS pixels at 100% scale.
const TASKBAR: u32 = 48;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    width: u32,
    height: u32,
    avail_width: u32,
    avail_height: u32,
    device_pixel_ratio: f64,
    hardware_concurrency: u32,
    timezone_offset: i32,
}

impl HardwareProfile {
    /// Picks one desktop configuration from the seed. The same seed and
    /// language always produce the same profile; no clock or RNG is involved.
    pub fn derive(seed: u64, language: &str) -> Result<Self> {
        let timezone_offset = timezone_for_language(language)?;
        let ((width, height), device_pixel_ratio, hardware_concurrency) =
            DESKTOPS[(splitmix64(seed) % DESKTOPS.len() as u64) as usize];
        Self::from_physical(
            width,
            height,
            device_pixel_ratio,
            hardware_concurrency,
            timezone_offset,
        )
    }

    /// Builds a profile from an explicit physical display size. Values are
    /// rejected outside the measured envelope instead of being clamped.
    pub fn from_physical(
        width: u32,
        height: u32,
        device_pixel_ratio: f64,
        hardware_concurrency: u32,
        timezone_offset: i32,
    ) -> Result<Self> {
        if !(1024..=7680).contains(&width) || !(600..=4320).contains(&height) {
            return Err(Error::InvalidInput(
                "display size outside the measured envelope",
            ));
        }
        if !(1.0..=3.0).contains(&device_pixel_ratio) {
            return Err(Error::InvalidInput(
                "device pixel ratio outside the measured envelope",
            ));
        }
        if !(4..=32).contains(&hardware_concurrency) {
            return Err(Error::InvalidInput(
                "core count outside the measured envelope",
            ));
        }
        if timezone_offset % 15 != 0 || !(-720..=840).contains(&timezone_offset) {
            return Err(Error::InvalidInput("invalid timezone offset"));
        }
        let css = |value: u32| ((f64::from(value) / device_pixel_ratio).round() as u32).max(1);
        let avail_height = css(height).saturating_sub(TASKBAR).max(1);
        Ok(Self {
            width: css(width),
            height: css(height),
            avail_width: css(width),
            avail_height,
            device_pixel_ratio,
            hardware_concurrency,
            timezone_offset,
        })
    }

    pub fn validate(&self) -> Result<()> {
        Self::from_physical(
            (f64::from(self.width) * self.device_pixel_ratio).round() as u32,
            (f64::from(self.height) * self.device_pixel_ratio).round() as u32,
            self.device_pixel_ratio,
            self.hardware_concurrency,
            self.timezone_offset,
        )
        .map(|_| ())
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn avail_width(&self) -> u32 {
        self.avail_width
    }
    pub fn avail_height(&self) -> u32 {
        self.avail_height
    }
    pub fn device_pixel_ratio(&self) -> f64 {
        self.device_pixel_ratio
    }
    pub fn hardware_concurrency(&self) -> u32 {
        self.hardware_concurrency
    }
    pub fn timezone_offset(&self) -> i32 {
        self.timezone_offset
    }
}

/// Reads a stable seed from the recovered persistent identifier. Accepts any
/// lowercase hex prefix so callers are not tied to the full 64-character form.
pub fn seed_from_persistent_id(persistent_id: &str) -> Result<u64> {
    let hex: String = persistent_id.chars().take(16).collect();
    if hex.len() < 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput(
            "persistent identifier cannot seed a profile",
        ));
    }
    u64::from_str_radix(&hex, 16).map_err(|_| Error::InvalidInput("invalid seed"))
}

/// Seed for an installation that has not persisted a device identifier yet.
/// Derived from the state directory so two installations do not share a
/// profile, and so the choice survives restarts before the first identity write.
pub fn seed_from_state_root(root: &std::path::Path) -> u64 {
    use md5::{Digest, Md5};
    let mut md5 = Md5::new();
    md5.update(root.to_string_lossy().as_bytes());
    let digest = md5.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes)
}

/// Chinese and Japanese markets resolve to a single offset; the rest map by
/// primary subtag. An unmapped language is an error rather than a silent UTC,
/// because a declared locale paired with the wrong offset is itself a signal.
fn timezone_for_language(language: &str) -> Result<i32> {
    let primary = language.split(['-', '_']).next().unwrap_or_default();
    let offset = match primary.to_ascii_lowercase().as_str() {
        "zh" => -480,
        "ja" | "ko" => -540,
        "en" => 300,
        "de" | "fr" | "it" | "nl" | "sv" | "pl" | "cs" => -60,
        "es" | "pt" => -60,
        "ru" | "uk" | "tr" => -180,
        "ar" | "fa" => -180,
        _ => {
            return Err(Error::InvalidInput(
                "no timezone applies to the declared language",
            ));
        }
    };
    Ok(offset)
}

fn splitmix64(state: u64) -> u64 {
    let state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_produces_identical_profile() {
        let first = HardwareProfile::derive(0x0123_4567_89ab_cdef, "zh-CN").unwrap();
        let second = HardwareProfile::derive(0x0123_4567_89ab_cdef, "zh-CN").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn distinct_seeds_cover_multiple_configurations() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..64 {
            let profile = HardwareProfile::derive(seed, "zh-CN").unwrap();
            seen.insert((
                profile.width(),
                profile.height(),
                profile.hardware_concurrency(),
            ));
        }
        assert!(
            seen.len() >= 4,
            "expected several desktop shapes, saw {seen:?}"
        );
    }

    #[test]
    fn derived_values_stay_inside_the_measured_envelope() {
        for seed in 0..512 {
            let profile = HardwareProfile::derive(seed, "zh-CN").unwrap();
            assert!((4..=32).contains(&profile.hardware_concurrency()));
            assert!(profile.width() >= 1024);
            assert!(profile.avail_width() <= profile.width());
            assert!(profile.avail_height() < profile.height());
            assert_eq!(profile.timezone_offset(), -480);
            profile.validate().unwrap();
        }
    }

    #[test]
    fn chinese_language_never_reports_utc() {
        for language in ["zh-CN", "zh-TW", "zh-HK", "zh"] {
            let profile = HardwareProfile::derive(7, language).unwrap();
            assert_eq!(profile.timezone_offset(), -480);
            assert_ne!(profile.timezone_offset(), 0);
        }
    }

    #[test]
    fn unmapped_language_is_rejected() {
        assert!(HardwareProfile::derive(1, "tlh").is_err());
        assert!(HardwareProfile::derive(1, "").is_err());
    }

    #[test]
    fn out_of_envelope_values_are_rejected() {
        assert!(HardwareProfile::from_physical(1920, 1080, 1.0, 2, -480).is_err());
        assert!(HardwareProfile::from_physical(1920, 1080, 1.0, 64, -480).is_err());
        assert!(HardwareProfile::from_physical(1920, 1080, 0.5, 8, -480).is_err());
        assert!(HardwareProfile::from_physical(1920, 1080, 1.0, 8, 7).is_err());
        assert!(HardwareProfile::from_physical(1920, 1080, 1.0, 8, 9000).is_err());
        assert!(HardwareProfile::from_physical(200, 100, 1.0, 8, -480).is_err());
    }

    #[test]
    fn css_pixels_follow_the_scale_factor() {
        let profile = HardwareProfile::from_physical(2560, 1440, 1.25, 16, -480).unwrap();
        assert_eq!((profile.width(), profile.height()), (2048, 1152));
        profile.validate().unwrap();
    }

    #[test]
    fn persistent_id_seeding_requires_hex() {
        assert!(seed_from_persistent_id("not-hex-at-all-here").is_err());
        assert!(seed_from_persistent_id("abc").is_err());
        assert!(seed_from_persistent_id("0123456789abcdef").is_ok());
    }
}
