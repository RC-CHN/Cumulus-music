//! Explicit local-file integration probe. Never run by `cargo test`.
use cumulus_core::{Error, Result, media::ncm::*};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

fn sha(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut block = [0u8; 65536];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: ncm_roundtrip <input.ncm> <new-output-directory>",
        ));
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let source_sha = sha(&input)?;
    let original = NcmReader::open(&input, HeaderPolicy::Strict)?;
    let info = original.info().clone();
    let cover = original.cover().to_vec();
    let suffix = match info.audio_format {
        Some(AudioFormat::Flac) => "flac",
        Some(AudioFormat::Mp3) => "mp3",
        Some(AudioFormat::M4a) => "m4a",
        None => return Err(Error::Media("unknown input audio")),
    };
    std::fs::create_dir(&output)?;
    let raw = output.join(format!("decoded.{suffix}"));
    let rewritten = output.join("rewrapped.ncm");
    let restored = output.join(format!("roundtrip.{suffix}"));
    let decoded = extract(&input, &raw, HeaderPolicy::Strict)?;
    let written = write_file(
        &raw,
        &rewritten,
        NcmWriteOptions {
            metadata: info.metadata.as_ref(),
            cover: &cover,
            audio_key: None,
        },
    )?;
    let mut reader = NcmReader::open(&rewritten, HeaderPolicy::Strict)?;
    if reader.info().metadata != info.metadata || reader.cover() != cover {
        return Err(Error::Media("roundtrip lost metadata or cover"));
    }
    let mut plain = File::open(&raw)?;
    let mut seek_checks = 0;
    for offset in [
        0,
        1,
        255,
        256,
        257,
        32767,
        info.audio_bytes.saturating_sub(13),
    ] {
        if offset > info.audio_bytes {
            continue;
        }
        let size = (info.audio_bytes - offset).min(521) as usize;
        let mut decoded = vec![0; size];
        let mut expected = vec![0; size];
        reader.seek(SeekFrom::Start(offset))?;
        plain.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut decoded)?;
        plain.read_exact(&mut expected)?;
        if decoded != expected {
            return Err(Error::Media("random seek mismatch"));
        }
        seek_checks += 1;
    }
    let final_report = extract(&rewritten, &restored, HeaderPolicy::Strict)?;
    if decoded.audio_sha256 != final_report.audio_sha256
        || written.audio_sha256 != decoded.audio_sha256
    {
        return Err(Error::Media("roundtrip audio changed"));
    }
    if source_sha != sha(&input)? {
        return Err(Error::Media("source file changed"));
    }
    Ok(
        json!({"schema":1,"implementation":"rust-core","input_sha256":source_sha,"audio_sha256":decoded.audio_sha256,"audio_bytes":info.audio_bytes,"format":info.audio_format,"cover_bytes":info.cover_bytes,"checks":{"strict_input_header":info.crc_verified,"source_unchanged":true,"writer_strict_header":reader.info().crc_verified,"metadata_equal":true,"cover_equal":true,"decoded_audio_equal":true,"random_seek":seek_checks},"output_directory":output}),
    )
}
fn main() {
    match run() {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).expect("report")),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
