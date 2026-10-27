//! Local inspection only: no account, HTTP request, entitlement or DSP activation.
use cumulus_core::effects::ncae::{self, Limits};
use std::{fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: effect_material_read INPUT [NEW_OUTPUT]")?;
    let output = args.next();
    if args.next().is_some() {
        return Err("usage: effect_material_read INPUT [NEW_OUTPUT]".into());
    }
    let decoded = ncae::open(input, Limits::default())?;
    if let Some(path) = output {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&decoded.plain)?;
        file.sync_all()?;
    }
    println!("{}", serde_json::to_string_pretty(&decoded.summary())?);
    Ok(())
}
