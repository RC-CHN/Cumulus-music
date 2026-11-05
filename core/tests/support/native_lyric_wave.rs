/// Exact native-open PCM geometry; separate from the existing library fixture.
pub fn native_stereo_wave(seconds: u32) -> Vec<u8> {
    let frames = 44_100 * seconds;
    let bytes = frames * 4;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend([1, 0, 2, 0]); // PCM and stereo
    wav.extend(44_100_u32.to_le_bytes());
    wav.extend(176_400_u32.to_le_bytes());
    wav.extend([4, 0, 16, 0]); // block alignment and sample depth
    wav.extend(b"data");
    wav.extend(bytes.to_le_bytes());
    for frame in 0..frames {
        let left = ((frame % 101) as i16 - 50) * 32;
        wav.extend(left.to_le_bytes());
        wav.extend((-left).to_le_bytes());
    }
    wav
}
