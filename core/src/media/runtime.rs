//! Windows installation lookup shared by native media tools.
use std::path::{Path, PathBuf};

pub(crate) fn windows_tool(names: &[&str]) -> Option<PathBuf> {
    let executable = std::env::current_exe().ok();
    // The media runtime used by standalone hosts ships mpv and ffprobe
    // together. This is a trusted native launch setting, never IPC data.
    let configured = std::env::var_os("MPV_DLL_DIR_CUMULUS").map(PathBuf::from);
    candidate(
        executable.as_deref().and_then(Path::parent),
        configured.as_deref(),
        names,
    )
}

fn candidate(sibling: Option<&Path>, configured: Option<&Path>, names: &[&str]) -> Option<PathBuf> {
    [sibling, configured]
        .into_iter()
        .flatten()
        .filter(|directory| directory.is_absolute())
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn configured_media_tools_require_an_absolute_dir_and_installation_wins() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("program");
        let fixture = temp.path().join("runtime-音频");
        fs::create_dir(&package).unwrap();
        fs::create_dir(&fixture).unwrap();
        for name in ["libmpv-2.dll", "ffprobe.exe"] {
            let runtime = fixture.join(name);
            fs::write(&runtime, b"candidate only, never loaded").unwrap();
            assert_eq!(
                candidate(Some(&package), Some(&fixture), &[name]),
                Some(runtime)
            );
            assert!(candidate(Some(&package), Some(Path::new("runtime")), &[name]).is_none());
            let sibling = package.join(name);
            fs::write(&sibling, b"candidate only, never loaded").unwrap();
            assert_eq!(
                candidate(Some(&package), Some(&fixture), &[name]),
                Some(sibling)
            );
        }
    }
}
