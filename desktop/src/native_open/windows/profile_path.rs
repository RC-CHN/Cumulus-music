//! Lexical path and native device-map admission; no filesystem access here.

const MAX_PROFILE_COMPONENTS: usize = 64;
const MAX_NT_PATH_UNITS: usize = u16::MAX as usize / 2;

pub(super) struct DrivePath<'a> {
    pub(super) drive: u8,
    pub(super) relative: &'a str,
}

pub(super) fn drive_path(value: &str) -> Option<DrivePath<'_>> {
    let value = value.strip_prefix(r"\\?\").unwrap_or(value);
    let [drive, b':', b'\\', ..] = *value.as_bytes() else {
        return None;
    };
    let relative = value[3..].strip_suffix('\\').unwrap_or(&value[3..]);
    if !drive.is_ascii_alphabetic()
        || value.encode_utf16().count() > MAX_NT_PATH_UNITS
        || relative.split('\\').count() > MAX_PROFILE_COMPONENTS
        || relative.split('\\').any(|component| {
            component.is_empty()
                || component.ends_with([' ', '.'])
                || component.encode_utf16().count() > 255
                || component.chars().any(|character| {
                    character.is_control()
                        || matches!(character, ':' | '/' | '<' | '>' | '"' | '|' | '?' | '*')
                })
        })
    {
        return None;
    }
    Some(DrivePath {
        drive: drive.to_ascii_uppercase(),
        relative,
    })
}

pub(super) fn first_fixed_device(mapping: &[u16]) -> Option<String> {
    let end = mapping.iter().position(|unit| *unit == 0)?;
    let target = String::from_utf16(&mapping[..end]).ok()?;
    // QueryDosDevice returns the current mapping first. Never consider a
    // stale previous local mapping behind a current Mup/SUBST mapping.
    let prefix = r"\Device\HarddiskVolume";
    let (head, number) = target.split_at_checked(prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix)
        || !(1..=10).contains(&number.len())
        || !number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(format!("{prefix}{number}\\"))
}

pub(super) fn is_volume_guid_root(value: &str) -> bool {
    let Some(guid) = value
        .strip_prefix(r"\\?\Volume{")
        .and_then(|tail| tail.strip_suffix(r"}\"))
    else {
        return false;
    };
    guid.len() == 36
        && guid.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ordinary_absolute_drive_components_enter_profile_resolution() {
        for path in [r"c:\用户\音 乐", r"\\?\C:\用户\音 乐\"] {
            let path = drive_path(path).unwrap();
            assert_eq!(path.drive, b'C');
            assert_eq!(path.relative, r"用户\音 乐");
        }
        for path in [
            r"\\server\share\Cumulus",
            r"\\?\UNC\server\share\Cumulus",
            r"\\.\C:\Cumulus",
            r"C:Cumulus",
            r"C:\",
            r"C:\Users\..\Cumulus",
            r"C:\Users\Cumulus.",
            r"C:\Users\\Cumulus",
            r"C:\Users\Cumulus:alternate",
            r"C:\Users/Cumulus",
        ] {
            assert!(drive_path(path).is_none(), "{path}");
        }
    }

    #[test]
    fn only_the_first_exact_native_volume_mapping_can_supply_a_root() {
        let native = |text: &str| text.encode_utf16().collect::<Vec<_>>();
        assert_eq!(
            first_fixed_device(&native("\\device\\harddiskvolume22\0\\Device\\Mup\0\0")),
            Some(r"\Device\HarddiskVolume22\".into()),
        );
        for mapping in [
            "\\Device\\Mup\\server\\share\0\\Device\\HarddiskVolume1\0\0",
            "\\Device\\LanmanRedirector\\host\\share\0\0",
            "\\??\\C:\\folder\0\0",
            "\\Device\\HarddiskVolume2\\folder\0\0",
            "\\Device\\HarddiskVolume-2\0\0",
            "\\Device\\HarddiskVolume12",
        ] {
            assert!(
                first_fixed_device(&native(mapping)).is_none(),
                "{mapping:?}"
            );
        }
        assert!(is_volume_guid_root(
            r"\\?\Volume{f1b44cdc-6170-4598-853a-4ca5d1eb4958}\"
        ));
        assert!(!is_volume_guid_root(
            r"\\?\Volume{f1b44cdc-6170-4598-853a-4ca5d1eb4958}\sub\"
        ));
    }
}
