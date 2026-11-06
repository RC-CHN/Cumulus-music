// The NT file name is relative to a real root handle. Keep the exact path
// spelling; Path::components would normalize some rejected components away.
const MAX_NAME_UNITS: usize = u16::MAX as usize / 2;

pub(super) fn relative_drive_path(path: &str) -> Option<(u8, &str)> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let [drive, b':', b'\\', ..] = *path.as_bytes() else {
        return None;
    };
    let relative = &path[3..];
    if !drive.is_ascii_alphabetic()
        || relative.len() > MAX_NAME_UNITS * 4
        || relative.encode_utf16().count() > MAX_NAME_UNITS
        || relative.split('\\').any(ineligible_component)
    {
        return None;
    }
    Some((drive.to_ascii_uppercase(), relative))
}

fn ineligible_component(component: &str) -> bool {
    component.is_empty()
        || component.ends_with([' ', '.'])
        || component.encode_utf16().count() > 255
        || component.chars().any(|ch| {
            ch.is_ascii_control() || matches!(ch, ':' | '/' | '<' | '>' | '"' | '|' | '?' | '*')
        })
}

pub(super) fn is_volume_guid_root(path: &str) -> bool {
    let Some(guid) = path
        .strip_prefix(r"\\?\Volume{")
        .and_then(|tail| tail.strip_suffix(r"}\"))
    else {
        return false;
    };
    guid.len() == 36
        && guid.bytes().enumerate().all(|(i, ch)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                ch == b'-'
            } else {
                ch.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unanchored_ambiguous_or_nondefault_stream_paths() {
        let relative = r"音乐\🪗.FLAC";
        assert_eq!(
            relative_drive_path(r"c:\音乐\🪗.FLAC"),
            Some((b'C', relative))
        );
        assert_eq!(
            relative_drive_path(r"\\?\C:\音乐\🪗.FLAC"),
            Some((b'C', relative))
        );
        for path in [
            r"\\host\share\track.flac",
            r"\\?\UNC\host\share\track.flac",
            r"\\.\C:\track.flac",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\track.flac",
            r"C:track.flac",
            r"C:\",
            r"C:\\track.flac",
            r"C:\a\.\track.flac",
            r"C:\a\..\track.flac",
            r"C:\a\track.flac:stream",
            r"C:/a/track.flac",
            r"C:\a\track.flac ",
            r"\\?\C:\a.\track.flac",
            "C:\\bad\0name.flac",
        ] {
            assert_eq!(relative_drive_path(path), None, "{path:?}");
        }
        assert!(relative_drive_path(&format!("C:\\{}", "🪗".repeat(128))).is_none());
        let component = "a".repeat(255);
        let long = std::iter::repeat_n(component, 129)
            .collect::<Vec<_>>()
            .join("\\");
        assert!(relative_drive_path(&format!("C:\\{long}")).is_none());
    }

    #[test]
    fn requires_a_real_volume_root_not_a_subst_subdirectory() {
        let root = r"\\?\Volume{12345678-9abC-DEf0-1234-567890abcdef}\";
        assert!(is_volume_guid_root(root));
        assert!(!is_volume_guid_root(&format!("{root}substituted")));
        assert!(!is_volume_guid_root(&root.replace("abcd", "xyz0")));
        assert!(!is_volume_guid_root(r"\\?\D:\"));
    }
}
