use super::{ffi, state::status};
use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicsBand {
    pub input_gain_db: f32,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_seconds: f32,
    pub release_seconds: f32,
    pub minimum_gain_db: f32,
    pub output_gain_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicsSettings {
    pub bands: [DynamicsBand; 5],
    pub crossover_hz: [f32; 4],
    pub output_gain_db: f32,
    pub process_lfe: bool,
    pub link_channels: bool,
    pub fourth_order: bool,
    pub band_count: u32,
    pub input_gain_db: f32,
}

/// Validated 171-byte native parameter block, stored independently of Rust/C
/// struct packing. Gain fields remain in dB until the native setter decodes it.
#[derive(Clone)]
pub struct DynamicsParameters([u8; 171]);

impl DynamicsParameters {
    pub fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; 171] = bytes
            .try_into()
            .map_err(|_| Error::Media("invalid dynamics parameter length"))?;
        // SAFETY: full byte array; validator performs no writes or state changes.
        status(unsafe { ffi::cumulus_dynamics_validate(bytes.as_ptr(), bytes.len()) })?;
        Ok(Self(bytes))
    }

    pub fn new(settings: DynamicsSettings) -> Result<Self> {
        let mut bytes = [0u8; 171];
        for (index, band) in settings.bands.iter().enumerate() {
            for (field, value) in [
                band.input_gain_db,
                band.threshold_db,
                band.ratio,
                band.attack_seconds,
                band.release_seconds,
                band.minimum_gain_db,
                band.output_gain_db,
            ]
            .iter()
            .enumerate()
            {
                bytes[index * 28 + field * 4..index * 28 + field * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        for (index, value) in settings.crossover_hz.iter().enumerate() {
            bytes[140 + index * 4..144 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[156..160].copy_from_slice(&settings.output_gain_db.to_le_bytes());
        bytes[160] = settings.process_lfe.into();
        bytes[161] = settings.link_channels.into();
        bytes[162] = settings.fourth_order.into();
        bytes[163..167].copy_from_slice(&settings.band_count.to_le_bytes());
        bytes[167..171].copy_from_slice(&settings.input_gain_db.to_le_bytes());
        Self::from_le_bytes(&bytes)
    }

    pub fn as_le_bytes(&self) -> &[u8; 171] {
        &self.0
    }

    pub fn settings(&self) -> DynamicsSettings {
        let read = |offset| {
            f32::from_le_bytes(
                self.0[offset..offset + 4]
                    .try_into()
                    .expect("checked block"),
            )
        };
        DynamicsSettings {
            bands: std::array::from_fn(|index| {
                let base = index * 28;
                DynamicsBand {
                    input_gain_db: read(base),
                    threshold_db: read(base + 4),
                    ratio: read(base + 8),
                    attack_seconds: read(base + 12),
                    release_seconds: read(base + 16),
                    minimum_gain_db: read(base + 20),
                    output_gain_db: read(base + 24),
                }
            }),
            crossover_hz: std::array::from_fn(|index| read(140 + index * 4)),
            output_gain_db: read(156),
            process_lfe: self.0[160] != 0,
            link_channels: self.0[161] != 0,
            fourth_order: self.0[162] != 0,
            band_count: u32::from_le_bytes(self.0[163..167].try_into().expect("checked block")),
            input_gain_db: read(167),
        }
    }
}
