use crate::{Error, Result};
use serde_json::Value;
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn drain(
    reader: impl Read + Send + 'static,
    limit: usize,
) -> std::io::Result<thread::JoinHandle<std::io::Result<Vec<u8>>>> {
    thread::Builder::new()
        .name("cumulus-probe-output".into())
        .spawn(move || {
            let mut data = Vec::new();
            reader.take((limit + 1) as u64).read_to_end(&mut data)?;
            Ok(data)
        })
}

pub(super) fn run(executable: &Path, path: &Path, timeout: Duration) -> Result<Value> {
    const ENTRIES: &str = concat!(
        "format=duration,bit_rate:",
        "format_tags=title,artist,album,track:",
        "stream=codec_name,codec_type,sample_rate,channels"
    );
    let mut command = Command::new(executable);
    command.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-show_entries",
        ENTRIES,
        "-of",
        "json",
    ]);
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGuard(command.spawn()?);
    const OUTPUT_LIMIT: usize = 1024 * 1024;
    let stdout = drain(child.0.stdout.take().expect("piped stdout"), OUTPUT_LIMIT)?;
    let stderr = drain(child.0.stderr.take().expect("piped stderr"), 128 * 1024)?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            child.0.kill()?;
            child.0.wait()?;
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    let bytes = stdout
        .join()
        .map_err(|_| Error::State("audio probe reader stopped"))??;
    let errors = stderr
        .join()
        .map_err(|_| Error::State("audio probe reader stopped"))??;
    if bytes.len() > OUTPUT_LIMIT || errors.len() > 128 * 1024 {
        return Err(Error::Media("audio probe output exceeds limit"));
    }
    let status = status.ok_or(Error::State("audio probe timed out"))?;
    if !status.success() {
        return Err(Error::Media("audio metadata could not be decoded"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Media("invalid audio probe response"))
}
