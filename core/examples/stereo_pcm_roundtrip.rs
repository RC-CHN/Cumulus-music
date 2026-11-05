//! Explicit synthetic held-source/native PCM proof; no GUI/physical claim.
#[path = "support/stereo_pcm_proof/mod.rs"]
mod proof;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: stereo_pcm_roundtrip TRUSTED_FLAC_LIBRARY NEW_PROOF_ROOT".into());
    }
    let report = proof::verify(
        std::path::Path::new(&args[0]),
        std::path::Path::new(&args[1]),
        |stage| eprintln!("{stage}"),
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
