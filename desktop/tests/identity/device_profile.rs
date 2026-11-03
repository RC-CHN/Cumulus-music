use super::*;
use cumulus_core::protocol::http::OFFICIAL_WINDOWS_USER_AGENT;

#[test]
fn default_and_exact_launch_overrides_select_construction_modes() {
    assert_eq!(
        IdentityMode::from_override(None).unwrap(),
        IdentityMode::WindowsAuto
    );
    for (value, expected) in [
        ("native", IdentityMode::Native),
        ("local", IdentityMode::Local),
        ("windows-local", IdentityMode::WindowsLocal),
    ] {
        assert_eq!(
            IdentityMode::from_override(Some(OsStr::new(value))).unwrap(),
            expected
        );
    }
    for value in ["", "windows", "Native", " local", "windows-local "] {
        assert!(IdentityMode::from_override(Some(OsStr::new(value))).is_err());
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_launch_override_is_rejected() {
    use std::os::unix::ffi::OsStrExt;
    assert!(IdentityMode::from_override(Some(OsStr::from_bytes(&[0xff]))).is_err());
}

#[test]
fn native_override_keeps_platform_default_without_collecting_or_opening_state() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("unopened-state");
    let actual = IdentityMode::Native
        .build(&root, |_, _, _| {
            panic!("native mode must not collect hardware")
        })
        .unwrap();
    let expected = ClientProfile::default();
    assert_eq!(actual.user_agent, expected.user_agent);
    assert_eq!(actual.os_version, expected.os_version);
    assert!(actual.client_sign.is_none());
    assert!(!root.exists());
}

#[cfg(target_os = "linux")]
fn synthetic_sysroot(root: &Path, mac: &str) {
    for (path, value) in [
        ("class/net/fixture0/address", mac),
        ("class/net/fixture0/operstate", "up"),
        ("block/vda/device/serial", "SYNTHETIC-DISK"),
        ("class/dmi/id/board_serial", "SYNTHETIC-BOARD"),
        ("class/dmi/id/sys_vendor", "Synthetic"),
        ("class/dmi/id/product_name", "Fixture"),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_uses_local_facts_with_windows_declarations_and_preserves_existing_ids() {
    use cumulus_core::{
        account::credentials::Credentials, protocol::http::ApiOrigin, storage::StateStore,
    };
    use serde_json::json;
    use std::sync::Arc;

    let directory = tempfile::tempdir().unwrap();
    let sysroot = directory.path().join("sys");
    let root = directory.path().join("state");
    synthetic_sysroot(&sysroot, "02:12:34:56:78:9A");
    let store = StateStore::open(&root).unwrap();
    let persistent_id = "a".repeat(64);
    let original_identity = json!({
        "persistent_id": persistent_id,
        "creation_source": {"platform": "synthetic-existing-installation"}
    });
    store
        .put("device", "identity", "client_sign", &original_identity)
        .unwrap();
    let session_store = Arc::new(StateStore::open(root.join("sessions/fixture")).unwrap());
    let credentials = Credentials::open(session_store.clone(), ApiOrigin::Pc).unwrap();
    let original_device_id = credentials.device_id().unwrap();
    let original_session = session_store
        .get("transport", "credentials", "session")
        .unwrap();

    for selection in [
        None,
        Some(OsStr::new("windows-local")),
        Some(OsStr::new("local")),
    ] {
        let mode = IdentityMode::from_override(selection).unwrap();
        let profile = mode
            .build(&root, |root, base, allow_missing| {
                local_from_sysroot(root, &sysroot, base, allow_missing)
            })
            .unwrap();
        if mode == IdentityMode::Local {
            assert_eq!(profile.user_agent, ClientProfile::default().user_agent);
            assert_eq!(profile.os_version, "Linux");
        } else {
            assert_eq!(profile.user_agent, OFFICIAL_WINDOWS_USER_AGENT);
            assert_eq!(
                profile.os_version,
                "Microsoft-Windows-10-Professional-build-19045-64bit"
            );
        }
        let sign = profile.client_sign.unwrap();
        assert!(sign.starts_with("02:12:34:56:78:9A@SYNTHETIC-DISK@SYNTHETIC-BOARD@"));
        assert!(sign.contains(&persistent_id));
        // New hardware observations never replace the previously persisted ID.
        synthetic_sysroot(&sysroot, "02:12:34:56:78:9B");
        let next = mode
            .build(&root, |root, base, allow_missing| {
                local_from_sysroot(root, &sysroot, base, allow_missing)
            })
            .unwrap();
        assert!(next.client_sign.unwrap().contains(&persistent_id));
        assert_eq!(
            store.get("device", "identity", "client_sign").unwrap(),
            Some(original_identity.clone())
        );
        assert_eq!(credentials.device_id().unwrap(), original_device_id);
        assert_eq!(
            session_store
                .get("transport", "credentials", "session")
                .unwrap(),
            original_session
        );
        synthetic_sysroot(&sysroot, "02:12:34:56:78:9A");
    }
    let observed = store
        .get("device", "identity", "last_observed")
        .unwrap()
        .unwrap();
    assert_eq!(observed["source"]["platform"], "linux-sysfs-adapter");
    assert_eq!(observed["parts"][3], persistent_id);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_falls_back_to_declarations_only_when_hardware_is_unavailable() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("unopened-state");
    let sysroot = directory.path().join("empty-sys");
    std::fs::create_dir(&sysroot).unwrap();
    let mode = IdentityMode::from_override(None).unwrap();
    let profile = mode
        .build(&root, |root, base, allow_missing| {
            local_from_sysroot(root, &sysroot, base, allow_missing)
        })
        .unwrap();
    assert_eq!(profile.user_agent, OFFICIAL_WINDOWS_USER_AGENT);
    assert_eq!(
        profile.os_version,
        "Microsoft-Windows-10-Professional-build-19045-64bit"
    );
    assert!(profile.client_sign.is_none());
    assert!(!root.exists());
    for mode in [IdentityMode::Local, IdentityMode::WindowsLocal] {
        assert!(matches!(
            mode.build(&root, |root, base, allow_missing| {
                local_from_sysroot(root, &sysroot, base, allow_missing)
            }),
            Err(Error::State("hardware identity unavailable"))
        ));
        assert!(!root.exists());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_does_not_fall_back_for_unrepresentable_hardware_text() {
    let directory = tempfile::tempdir().unwrap();
    let sysroot = directory.path().join("sys");
    let root = directory.path().join("unopened-state");
    synthetic_sysroot(&sysroot, "02:12:34:56:78:9A");
    std::fs::write(sysroot.join("class/dmi/id/sys_vendor"), "\u{1f3b5}").unwrap();
    assert!(matches!(
        IdentityMode::WindowsAuto.build(&root, |root, base, allow_missing| {
            local_from_sysroot(root, &sysroot, base, allow_missing)
        }),
        Err(Error::State(
            "hardware text cannot use recovered GBK encoding"
        ))
    ));
    assert!(!root.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_propagates_corrupt_storage_without_falling_back() {
    let directory = tempfile::tempdir().unwrap();
    let sysroot = directory.path().join("sys");
    let root = directory.path().join("state");
    synthetic_sysroot(&sysroot, "02:12:34:56:78:9A");
    std::fs::create_dir(&root).unwrap();
    let path = root.join("core.sqlite3");
    let corrupt = b"synthetic invalid sqlite database";
    std::fs::write(&path, corrupt).unwrap();
    assert!(matches!(
        IdentityMode::WindowsAuto.build(&root, |root, base, allow_missing| {
            local_from_sysroot(root, &sysroot, base, allow_missing)
        }),
        Err(Error::Storage(_))
    ));
    assert_eq!(std::fs::read(path).unwrap(), corrupt);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_default_does_not_hide_invalid_persisted_identity_or_failed_observation() {
    use cumulus_core::storage::StateStore;
    use serde_json::json;

    let directory = tempfile::tempdir().unwrap();
    let sysroot = directory.path().join("sys");
    let root = directory.path().join("state");
    synthetic_sysroot(&sysroot, "02:12:34:56:78:9A");
    let store = StateStore::open(&root).unwrap();
    let invalid = json!({"persistent_id": "synthetic-invalid"});
    store
        .put("device", "identity", "client_sign", &invalid)
        .unwrap();
    assert!(matches!(
        IdentityMode::WindowsAuto.build(&root, |root, base, allow_missing| {
            local_from_sysroot(root, &sysroot, base, allow_missing)
        }),
        Err(Error::State(
            "stored persistent identity is invalid; rotation refused"
        ))
    ));
    assert_eq!(
        store.get("device", "identity", "client_sign").unwrap(),
        Some(invalid)
    );
    assert!(
        store
            .get("device", "identity", "last_observed")
            .unwrap()
            .is_none()
    );
    store.delete("device", "identity", "client_sign").unwrap();
    let database = rusqlite::Connection::open(root.join("core.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER reject_observation BEFORE INSERT ON state
         WHEN NEW.kind='identity' AND NEW.key='last_observed'
         BEGIN SELECT RAISE(ABORT,'synthetic fixture'); END;",
        )
        .unwrap();
    assert!(matches!(
        IdentityMode::WindowsAuto.build(&root, |root, base, allow_missing| {
            local_from_sysroot(root, &sysroot, base, allow_missing)
        }),
        Err(Error::Storage(_))
    ));
    assert!(store.values("device", "identity").unwrap().is_empty());
}

#[cfg(target_os = "windows")]
#[test]
fn windows_default_and_windows_local_use_declarations_without_a_hardware_signature() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("unopened-state");
    for selection in [None, Some(OsStr::new("windows-local"))] {
        let profile = IdentityMode::from_override(selection)
            .unwrap()
            .build(&root, |_, _, _| {
                panic!("Windows must not invoke the Linux collector")
            })
            .unwrap();
        assert_eq!(profile.user_agent, OFFICIAL_WINDOWS_USER_AGENT);
        assert_eq!(
            profile.os_version,
            "Microsoft-Windows-10-Professional-build-19045-64bit"
        );
        assert!(profile.client_sign.is_none());
    }
    assert!(!root.exists());
}

#[cfg(not(target_os = "linux"))]
#[test]
fn explicit_local_mode_still_reports_unsupported_native_collection() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("unopened-state");
    assert!(matches!(
        IdentityMode::Local.build(&root, local),
        Err(Error::State(_))
    ));
    assert!(!root.exists());
}
