//! Local JSON parameter composition; no account, Python, Node or DSP runtime.
use cumulus_core::effects::compose;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Read;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuiltinInput {
    song_json: Option<Value>,
    enhancements: Vec<compose::Enhancement>,
    #[serde(default)]
    support_stereo: bool,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut input)?;
    if input.len() > 4 * 1024 * 1024 {
        return Err("input exceeds limit".into());
    }
    let value: Value = serde_json::from_slice(&input)?;
    let mut args = std::env::args().skip(1);
    let mode = args.next();
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let result = match mode.as_deref() {
        None => compose::compose(&value)?,
        Some("--builtin") => {
            let input: BuiltinInput = serde_json::from_value(value)?;
            compose::compose_builtin(
                input.song_json.as_ref(),
                &input.enhancements,
                input.support_stereo,
            )?
        }
        _ => return Err("usage: effect_compose [--builtin] < parameters.json".into()),
    };
    let mut output = serde_json::to_value(result)?;
    output["success"] = json!(true);
    output["errorMsg"] = json!("");
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
