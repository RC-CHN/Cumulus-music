//! Explicit FFprobe/NCM/CUE file integration; statistics only, no index writes.
use cumulus_core::{
    Error, Result,
    library::{
        cue::parse_cue,
        probe::{AudioProbe, Ffprobe},
    },
};
use serde_json::{Value, json};
use std::path::PathBuf;

fn inspect(path: PathBuf, probe: &Ffprobe) -> Result<Value> {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("cue"))
    {
        let sheet = parse_cue(path, probe)?;
        Ok(
            json!({"ok":true,"kind":"cue","sources":sheet.sources.len(),"tracks":sheet.tracks.iter().map(|t|json!({"track":t.track,"offset":t.offset,"duration":t.duration,"source_duration":t.source_duration})).collect::<Vec<_>>()}),
        )
    } else {
        let metadata = probe.inspect(&path)?;
        Ok(
            json!({"ok":true,"kind":"audio","format":metadata.format,"container":metadata.container,"duration":metadata.duration,"sample_rate":metadata.sample_rate,"channels":metadata.channels,"cover_bytes":metadata.cover_bytes}),
        )
    }
}
fn run() -> Result<Value> {
    let paths: Vec<_> = std::env::args().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        return Err(Error::InvalidInput(
            "usage: local_media_read <audio-or-cue>...",
        ));
    }
    let probe = Ffprobe::default();
    let checks: Vec<_> = paths
        .into_iter()
        .map(|path| match inspect(path, &probe) {
            Ok(value) => value,
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        })
        .collect();
    Ok(json!({"schema":1,"implementation":"rust-core","checks":checks}))
}
fn main() {
    match run() {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).expect("report"));
            if report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["ok"] != true)
            {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
