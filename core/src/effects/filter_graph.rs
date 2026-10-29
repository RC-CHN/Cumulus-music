use std::path::Path;

/// Trusted native library path, never accepted from page/API data. FFmpeg has
/// two tokenizers; mpv supplies its own separate length-prefixed outer quoting.
pub(super) fn graph(path: &Path, plugin: &str, capacity: u32, id: u32) -> String {
    fn escape(value: &str, special: &str) -> String {
        let mut result = String::new();
        for c in value.chars() {
            if special.contains(c) {
                result.push('\\');
            }
            result.push(c);
        }
        result
    }
    let path = escape(path.to_str().expect("checked native path"), "\\': ");
    let path = escape(&path, "\\'[],; ");
    // Stable frame sizes preserve recovered recurrences and transition edges.
    // Do not pad EOF: preserve duration and the actual final partial block.
    format!("asetnsamples=n={capacity}:p=0,ladspa=file={path}:plugin={plugin}:controls={id}")
}
