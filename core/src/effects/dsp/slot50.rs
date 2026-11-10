//! The legacy SDK processor's local equalizer tone node (slot +0x50).
//! The standalone BassTreble class has different filters and is independent.

use super::{Format, ParametricBand, ParametricEq, ParametricKind, PlanarBlock};
use crate::{Error, Result};

/// The old local equalizer's bounded whole-material tone values.
/// Two zero values disable this component, as in the original UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Slot50Material {
    bass_db: i32,
    treble_db: i32,
}

impl Slot50Material {
    pub fn new(bass_db: i32, treble_db: i32) -> Result<Self> {
        if !(-10..=10).contains(&bass_db) || !(-10..=10).contains(&treble_db) {
            return Err(Error::InvalidInput("slot 50 tone gain outside -10..10 dB"));
        }
        Ok(Self { bass_db, treble_db })
    }
}

/// Normalized planar f32 leaf only. The caller owns transport and supplies
/// full initialized capacity even for short blocks. This does not select
/// playback sources, convert sample formats, group packets, or drain tails.
pub struct Slot50Tone(ParametricEq);

impl Slot50Tone {
    /// Original defaults and explicit device Reset, with no PCM processed.
    pub fn new(format: Format) -> Result<Self> {
        if !matches!(format.rate(), 44_100.0 | 48_000.0)
            || !matches!(format.channels(), 1 | 2)
            || format.capacity() != 2048
        {
            return Err(Error::InvalidInput("unsupported slot 50 tone format"));
        }
        let mut tone = Self(ParametricEq::new(format, false, true)?);
        tone.shelves(0.0, 0.0)?;
        for index in [1, 3, 4, 5, 6, 7, 8] {
            let (kind, frequency, q) = if index == 1 {
                (ParametricKind::Peak, 1000.0, 1.0)
            } else {
                (ParametricKind::HighShelfQ, 5000.0, 5.0)
            };
            tone.0.set_band(
                index,
                ParametricBand {
                    kind,
                    gain_db: 0.0,
                    frequency,
                    q,
                    enabled: false,
                },
            )?;
        }
        tone.master_bypass()?;
        tone.reset()?;
        Ok(tone)
    }

    pub fn format(&self) -> Format {
        self.0.format()
    }

    /// Tone portion of a full local-material application. 1BBA0 first
    /// sets target bypass and flat shelves; subsequent setters write both
    /// requested gains and reopen the component if either is nonzero.
    pub fn apply_material(&mut self, material: Slot50Material) -> Result<()> {
        self.master_bypass()?;
        self.shelves(0.0, 0.0)?;
        if material != Slot50Material::default() {
            self.shelves(material.bass_db as f32, material.treble_db as f32)?;
            self.0.set_bypass(false)?;
        }
        Ok(())
    }

    /// Modern EF8CA0 dispatch has an independent on flag. Both neutral
    /// shelves must still run when it explicitly enables the node.
    pub(crate) fn apply_enabled_modern(&mut self, bass_db: f32, treble_db: f32) -> Result<()> {
        self.shelves(bass_db, treble_db)?;
        self.0.set_bypass(false)
    }

    /// Tone target of a master-off action. Keep the current coefficients,
    /// histories, and previous bypass state for the next native blend.
    pub fn master_bypass(&mut self) -> Result<()> {
        self.0.set_bypass(true)
    }

    /// Native reset clears filter histories and settles previous to target
    /// without generating a block or resetting the material.
    pub fn reset(&mut self) -> Result<()> {
        self.0.reset()
    }

    /// Process one actual leaf call. Valid frame count and caller EOF are
    /// unchanged. Capacity padding participates in native bypass blends.
    pub fn process(&mut self, block: &mut PlanarBlock, valid_frames: usize) -> Result<()> {
        self.0.process(block, valid_frames)
    }

    fn shelves(&mut self, bass_db: f32, treble_db: f32) -> Result<()> {
        for (index, kind, frequency, gain_db) in [
            (0, ParametricKind::LowShelfQ, 120.0, bass_db),
            (2, ParametricKind::HighShelfQ, 6000.0, treble_db),
        ] {
            self.0.set_band(
                index,
                ParametricBand {
                    kind,
                    gain_db,
                    frequency,
                    q: 0.5,
                    enabled: true,
                },
            )?;
        }
        Ok(())
    }
}
