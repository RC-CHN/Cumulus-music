//! Synthetic uncompressed float WAV for offline decoder verification.
use cumulus_core::Result;
use std::{io::Write, path::Path};
pub fn wav(path: &Path, rate: u32, channels: usize, frames: usize) -> Result<Vec<f32>> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let count = frames * channels;
    let bytes = (count * 4) as u32;
    for part in [
        b"RIFF".to_vec(),
        (36 + bytes).to_le_bytes().to_vec(),
        b"WAVEfmt ".to_vec(),
        16u32.to_le_bytes().to_vec(),
        3u16.to_le_bytes().to_vec(),
        (channels as u16).to_le_bytes().to_vec(),
        rate.to_le_bytes().to_vec(),
        (rate * channels as u32 * 4).to_le_bytes().to_vec(),
        (channels as u16 * 4).to_le_bytes().to_vec(),
        32u16.to_le_bytes().to_vec(),
        b"data".to_vec(),
        bytes.to_le_bytes().to_vec(),
    ] {
        file.write_all(&part)?;
    }
    let mut samples = Vec::with_capacity(count);
    for i in 0..frames {
        for ch in 0..channels {
            let value = ((i as f64 * (137.0 + ch as f64 * 431.0) * std::f64::consts::TAU
                / rate as f64)
                .sin()
                * 0.02) as f32;
            samples.push(value);
            file.write_all(&value.to_le_bytes())?;
        }
    }
    Ok(samples)
}
