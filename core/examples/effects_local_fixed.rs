//! Explicit bounded PCM executable for the recovered fixed local scope.
//! No services, audio devices, player state, remote presets or source grants.
use cumulus_core::effects::{
    dsp::{Format, PlanarBlock},
    stereo::{LocalFixedStereoMaterial, LocalStereoMaterialPlan},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 5 {
        return Err(
            "usage: effects_local_fixed RATE CAPACITY PLAN.json INPUT.f32le OUTPUT.f32le".into(),
        );
    }
    let rate = args[0].parse::<u32>()?;
    let capacity = args[1].parse::<u32>()?;
    if fs::metadata(&args[2])?.len() > 64 * 1024 || fs::metadata(&args[3])?.len() > 8 * 1024 * 1024
    {
        return Err("bounded local PCM input exceeded".into());
    }
    let plan_bytes = fs::read(&args[2])?;
    let input = fs::read(&args[3])?;
    if plan_bytes.len() > 64 * 1024
        || input.len() > 8 * 1024 * 1024
        || !input.len().is_multiple_of(8)
    {
        return Err("invalid bounded stereo PCM input".into());
    }
    let json = serde_json::from_slice(&plan_bytes)?;
    let plan = LocalStereoMaterialPlan::from_equalizer_json(&json)?;
    let format = Format::new(rate as f32, 2, capacity)?;
    let mut graph = LocalFixedStereoMaterial::new(format, plan)?;
    let source = input
        .chunks_exact(4)
        .map(|raw| f32::from_le_bytes(raw.try_into().expect("four bytes")))
        .collect::<Vec<_>>();
    if source.iter().any(|value| !value.is_finite()) {
        return Err("non-finite input PCM".into());
    }
    let mut block = PlanarBlock::new(format);
    let mut cursor = 0;
    let mut rendered = Vec::new();
    let mut done = false;
    let budget = (source.len() / 2).div_ceil(format.capacity())
        + (rate as usize).div_ceil(format.capacity())
        + 32;
    for _ in 0..budget {
        let samples = (source.len() - cursor).min(format.sample_count());
        let frames = block.load_interleaved(&source[cursor..cursor + samples])?;
        cursor += samples;
        let flow = graph.process(&mut block, frames, cursor == source.len())?;
        for frame in 0..flow.valid_frames {
            rendered.extend_from_slice(&block.samples()[frame].to_le_bytes());
            rendered.extend_from_slice(&block.samples()[format.capacity() + frame].to_le_bytes());
        }
        if flow.end {
            done = true;
            break;
        }
    }
    if !done {
        return Err("bounded fixed graph did not reach actual EOF".into());
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[4])?;
    output.write_all(&rendered)?;
    output.sync_all()?;
    println!(
        "sourceFrames={} outputFrames={} rate={} channels=2; fixed local scope only; no live/audio/UI claim",
        source.len() / 2,
        rendered.len() / 8,
        rate
    );
    Ok(())
}
