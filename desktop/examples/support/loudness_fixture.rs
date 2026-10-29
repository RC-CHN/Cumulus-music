//! Generated PCM and official-format comments; no downloaded audio or account.
use cumulus_core::media::tags::encode_comment;
use serde_json::json;
use std::{
    fs,
    io::{BufWriter, Write},
    path::Path,
};

pub fn media(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir(root.join("media"))?;
    for (name, gain) in [
        ("01 有音量信息", Some(-18)),
        ("02 无音量信息", None),
        ("03 异常音量信息", Some(10000)),
    ] {
        let mut comment = Vec::new();
        if let Some(gain) = gain {
            let text = encode_comment(&format!(
                "music:{}",
                json!({"musicId":1,"musicName":name,"volumeDelta":gain,"format":"wav"})
            ))?;
            let mut text = text.into_bytes();
            text.push(0);
            let length = text.len() as u32;
            if text.len() % 2 != 0 {
                text.push(0);
            }
            comment.extend_from_slice(b"LIST");
            comment.extend_from_slice(&(12 + text.len() as u32).to_le_bytes());
            comment.extend_from_slice(b"INFOICMT");
            comment.extend_from_slice(&length.to_le_bytes());
            comment.extend(text);
        }
        let rate = 48000u32;
        let frames = rate * 90;
        let bytes = frames * 8;
        let mut file = BufWriter::new(fs::File::create_new(
            root.join("media").join(format!("{name}.wav")),
        )?);
        for part in [
            b"RIFF".to_vec(),
            (bytes + 36 + comment.len() as u32).to_le_bytes().to_vec(),
            b"WAVEfmt ".to_vec(),
            16u32.to_le_bytes().to_vec(),
            3u16.to_le_bytes().to_vec(),
            2u16.to_le_bytes().to_vec(),
            rate.to_le_bytes().to_vec(),
            (rate * 8).to_le_bytes().to_vec(),
            8u16.to_le_bytes().to_vec(),
            32u16.to_le_bytes().to_vec(),
            b"data".to_vec(),
            bytes.to_le_bytes().to_vec(),
        ] {
            file.write_all(&part)?;
        }
        for frame in 0..frames {
            for frequency in [440.0, 1357.0] {
                let value = ((frame as f64 * frequency * std::f64::consts::TAU / rate as f64).sin()
                    * 0.02) as f32;
                file.write_all(&value.to_le_bytes())?;
            }
        }
        file.write_all(&comment)?;
        file.flush()?;
    }
    Ok(())
}
