//! Offline, synthetic Watchman migration proof. No SDK, network or environment
//! collection; no account data. All outputs go into a fresh private directory.
use cumulus_core::{Result, storage::StateStore, watchman::*};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
};
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(root.join(name))?;
    file.write_all(serde_json::to_string_pretty(value)?.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
fn run(root: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(root)?;
    let conf = json!({"pn":"YD00000558929251","edc":true,"dtvp":86400000,"tto":2000,"ret":1});
    let encoded = encode_text(&conf.to_string())?;
    let config = Config::from_conf("YD00000558929251", "", &encoded)?;
    let nonce = request_nonce(1792865000123, [0, 127, 128, 255, 1, 2, 3, 4]);
    let fields = vec![
        ("bc", json!("200"), false),
        ("Kb", json!(config.product()), false),
        ("Ma", json!(nonce), false),
        ("Lb", json!(1), false),
        ("$b", json!("synthetic-offline-environment"), false),
        ("_move", json!([1, 100, 1, 20, 30]), false),
        ("Jb", Value::Null, true),
    ];
    let bytes = encode_fields(&fields)?;
    let packet = encode_packet(&bytes, [0, 127, 128, 255])?;
    if decode_packet(&packet)? != bytes {
        return Err(cumulus_core::Error::State("packet mismatch"));
    }
    let state_dir = root.join("state");
    let db = Arc::new(StateStore::open(&state_dir)?);
    let store = DeviceStore::new(db.clone(), config.clone());
    let device_response = json!([
        200,
        null,
        "synthetic-tid",
        "synthetic-did",
        null,
        "synthetic-ni"
    ]);
    store.apply_response(0, ReportKind::Device, &device_response, 1000, None)?;
    let first = store.snapshot()?;
    let reopened = DeviceStore::new(Arc::new(StateStore::open(&state_dir)?), config.clone());
    let second = reopened.snapshot()?;
    if first.did_native() != second.did_native() || !second.ready(&config, 1001) {
        return Err(cumulus_core::Error::State("state did not survive reopen"));
    }
    let mut traces = Vec::new();
    for sequence in [
        vec![Some(200)],
        vec![Some(420), None],
        vec![Some(470)],
        vec![],
    ] {
        let mut attempt = BehaviorAttempt::new(
            config.clone(),
            "synthetic-business",
            nonce.clone(),
            1000,
            None,
        )?;
        let mut result = None;
        for code in &sequence {
            match attempt.response(*code, second.did_native())? {
                BehaviorStep::Token(token) => {
                    result = Some(
                        json!({"token":token.encoded_native(),"status":format!("{:?}",token.status())}),
                    )
                }
                BehaviorStep::IllegalBusiness => result = Some(json!({"error":"illegal-business"})),
                _ => {}
            }
        }
        if sequence.is_empty()
            && let BehaviorStep::Token(token) = attempt.timeout(3000, second.did_native())?
        {
            result = Some(
                json!({"token":token.encoded_native(),"status":format!("{:?}",token.status())}),
            );
        }
        traces.push(json!({"responses":sequence,"result":result}));
    }
    save(
        root,
        "evidence.json",
        &json!({"sourceSha256":SOURCE_SHA256,"configuration":conf,"encodedConfiguration":encoded,"timestampMs":1792865000123_u64,"nonceRandom":"007f80ff01020304","nonce":nonce,"fields":fields,"payloadHex":hex::encode(bytes),"packetSeed":"007f80ff","packet":packet,"deviceResponse":device_response,"deviceObservedMs":1000,"traces":traces,"ndInfo":second.nd_info(&config)?}),
    )?;
    let report = json!({"ok":true,"scope":"synthetic offline Rust codec, state, callback and SQLite reopen", "networkRequests":0,"browserCollection":false,"sdkProviderConnected":false,"serverAcceptanceTested":false,"reopenedState":true,"traceCount":traces.len()});
    save(root, "report.json", &report)?;
    println!("{report}");
    Ok(())
}
fn main() -> Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or(cumulus_core::Error::InvalidInput(
            "usage: watchman_offline NEW_PRIVATE_DIRECTORY",
        ))?;
    run(Path::new(&root))
}
