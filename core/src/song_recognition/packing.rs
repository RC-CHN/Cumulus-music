use crate::{Error, Result};

const RATE: f32 = 8_000.0;
const HEADER_LEN: usize = 33;
const PEAK_LEN: usize = 12;

/// The native peak writer's two matrix coordinates and a raw f32 bit pattern.
///
/// Signal processing determines the coordinates, value and order. Packing
/// copies them as-is, with no rounding, sorting or filtering.
pub struct PeakRecord {
    pub bin: i32,
    pub frame: i32,
    pub value_bits: u32,
}

/// Packs the default PC recognizer record before zlib compression.
///
/// This is the observed constructor configuration: empty labels, unset header
/// flag and zero secondary header value. `sample_count` counts signed i16
/// samples in the 8 kHz mono buffer, not bytes or device input frames.
pub fn pack_default_packet(sample_count: u32, peaks: &[PeakRecord]) -> Result<Vec<u8>> {
    let sample_count = i32::try_from(sample_count)
        .map_err(|_| Error::InvalidInput("recognition sample count exceeds the native range"))?;
    let peak_count = i32::try_from(peaks.len())
        .map_err(|_| Error::InvalidInput("recognition peak count exceeds the native range"))?;
    let capacity = peaks
        .len()
        .checked_mul(PEAK_LEN)
        .and_then(|size| size.checked_add(HEADER_LEN))
        .ok_or(Error::InvalidInput("recognition packet is too large"))?;
    let mut packed = Vec::new();
    packed
        .try_reserve_exact(capacity)
        .map_err(|_| Error::InvalidInput("recognition packet is too large"))?;

    // D0C5A0: empty string length, normalized flag, secondary f32, duration f32.
    packed.extend_from_slice(&[0; 12]);
    packed.extend_from_slice(&(sample_count as f32 / RATE).to_le_bytes());
    // D0F4C0: five literal NUL bytes, empty label length, literal Peak, count.
    packed.extend_from_slice(&[0; 9]);
    packed.extend_from_slice(b"Peak");
    packed.extend_from_slice(&peak_count.to_le_bytes());
    for peak in peaks {
        packed.extend_from_slice(&peak.bin.to_le_bytes());
        packed.extend_from_slice(&peak.frame.to_le_bytes());
        packed.extend_from_slice(&peak.value_bits.to_le_bytes());
    }
    Ok(packed)
}
