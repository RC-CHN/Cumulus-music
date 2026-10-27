use cumulus_core::{Error, identity::*, protocol::http::ClientProfile, storage::StateStore};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, Barrier},
};
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/device-identity-native.json")).unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    hex::decode(value.as_str().unwrap()).unwrap()
}
fn facts(mac: &str) -> HardwareFacts {
    HardwareFacts::from_encoded(
        [mac.into(), "DISK-EXAMPLE".into(), "BOARD-EXAMPLE".into()],
        format!("{mac}DISK-EXAMPLE").into_bytes(),
        bytes(&fixture()["host"]),
        IdentitySource {
            platform: "synthetic-test".into(),
            interface: "test0".into(),
            disk_device: "testdisk".into(),
            host_description_available: true,
        },
    )
    .unwrap()
}
fn fact(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn net(root: &Path, name: &str, address: &str, physical: bool, active: bool) {
    fact(root, &format!("class/net/{name}/address"), address);
    fact(
        root,
        &format!("class/net/{name}/operstate"),
        if active { "up" } else { "down" },
    );
    if physical {
        fs::create_dir(root.join(format!("class/net/{name}/device"))).unwrap();
    }
}
fn repair_digest(bytes: &mut [u8]) {
    let digest = Md5::digest(&bytes[24..]);
    bytes[8..24].copy_from_slice(&digest);
}

#[test]
fn recovered_composition_and_double_digest_match_independent_native_evidence_vectors() {
    let f = fixture();
    assert_eq!(hex::encode(STORAGE_KEY), f["key"]);
    assert_eq!(MARKER, bytes(&f["marker"]));
    assert_eq!(ID_RECORD_KEY, f["idKey"].as_u64().unwrap() as u32);
    assert_eq!(
        derive_persistent_id(&bytes(&f["hardware"]), &bytes(&f["host"])),
        f["id"]
    );
    for row in f["compositions"].as_array().unwrap() {
        let parts = row["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            client_sign(
                parts.try_into().unwrap(),
                row["mask"].as_u64().unwrap() as u8
            )
            .unwrap(),
            row["value"]
        );
    }
    assert_eq!(client_sign(["MAC", "", "", "ID"], 255).unwrap(), "MAC@@@ID");
    assert!(client_sign(["bad\n", "", "", "ID"], 15).is_err());
}
#[test]
fn cbc_and_detached_tail_match_150_independent_openssl_vectors() {
    for row in fixture()["vectors"].as_array().unwrap() {
        let p = bytes(&row["plain"]);
        let c = bytes(&row["cipher"]);
        let r = bytes(&row["record"]);
        assert_eq!(storage_transform(&p, false).unwrap(), c);
        assert_eq!(storage_transform(&c, true).unwrap(), p);
        assert_eq!(encode_record(&p).unwrap(), r);
        assert_eq!(decode_record(&r).unwrap(), p);
    }
    for n in [0, 1, 17, 33, 65] {
        assert!(storage_transform(&vec![1; n], false).is_err());
        assert!(storage_transform(&vec![1; n], true).is_err());
    }
}
#[test]
fn native_container_decode_canonical_export_and_unknown_raw_records_are_distinct() {
    let f = fixture();
    let native = bytes(&f["container"]);
    let records = parse_container(&native).unwrap();
    assert_eq!(
        decode_record(records[&ID_RECORD_KEY]).unwrap(),
        f["id"].as_str().unwrap().as_bytes()
    );
    let mut owned = records
        .into_iter()
        .map(|(k, v)| (k, v.to_vec()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        encode_container(&owned).unwrap(),
        bytes(&f["canonicalContainer"])
    );
    owned.insert(0x123456, b"raw-not-encrypted".to_vec());
    let exported = encode_container(&owned).unwrap();
    assert_eq!(
        parse_container(&exported).unwrap()[&0x123456],
        b"raw-not-encrypted"
    );
    let report = inspect_container(&exported).unwrap();
    assert!(
        report
            .iter()
            .find(|r| r.key == ID_RECORD_KEY)
            .unwrap()
            .derived_suffix_valid
            .unwrap()
    );
    assert!(
        report
            .iter()
            .find(|r| r.key == MARKER_RECORD_KEY)
            .unwrap()
            .known_marker
            .unwrap()
    );
    assert!(
        report
            .iter()
            .find(|r| r.key == 0x123456)
            .unwrap()
            .digest_valid
            .is_none()
    );
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains(f["id"].as_str().unwrap())
    );
}
#[test]
fn malformed_containers_and_digest_failures_do_not_delete_or_silently_replace_files() {
    let original = bytes(&fixture()["container"]);
    let mut malformed = vec![
        vec![],
        original[..27].to_vec(),
        original[..original.len() - 1].to_vec(),
    ];
    for (offset, value) in [
        (0, 0),
        (4, 0),
        (24, u32::MAX),
        (32, 0),
        (36, u32::MAX),
        (40, ID_RECORD_KEY),
        (44, 52),
    ] {
        let mut b = original.clone();
        b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        repair_digest(&mut b);
        malformed.push(b);
    }
    for b in malformed {
        assert!(parse_container(&b).is_err());
    }
    let record = encode_record(MARKER).unwrap();
    for pos in [0, 16, record.len() - 1] {
        let mut bad = record.clone();
        bad[pos] ^= 1;
        assert!(decode_record(&bad).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("isc.dat");
    let bad = b"bad device record";
    fs::write(&path, bad).unwrap();
    assert!(read_container(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), bad);
    fs::write(&path, &original).unwrap();
    assert_eq!(read_container(&path).unwrap(), original);
    assert!(read_container(root.path()).is_err());
}
#[test]
fn linux_adapter_prefers_physical_then_active_and_preserves_missing_components() {
    let root = tempfile::tempdir().unwrap();
    net(root.path(), "docker0", "02:00:00:00:00:01", false, true);
    net(root.path(), "eno0", "10:20:30:40:50:60", true, false);
    net(root.path(), "eno1", "10:20:30:40:50:61", true, true);
    net(root.path(), "bad", "00:00:00:00:00:01", true, true);
    let f = collect_linux_identity(root.path()).unwrap();
    let store = Arc::new(StateStore::open(root.path().join("state")).unwrap());
    let d = IdentityService::new(store).resolve(&f).unwrap();
    assert!(d.client_sign().unwrap().starts_with("10:20:30:40:50:61@@@"));
    assert!(!d.summary().disk_available && !d.summary().board_available);
    fs::remove_dir_all(root.path().join("class/net/eno1")).unwrap();
    let d = IdentityService::new(Arc::new(
        StateStore::open(root.path().join("state")).unwrap(),
    ))
    .resolve(&collect_linux_identity(root.path()).unwrap())
    .unwrap();
    assert!(d.client_sign().unwrap().starts_with("10:20:30:40:50:60@@@"));
}
#[test]
fn linux_serial_normalization_gbk_host_and_missing_or_unrepresentable_facts() {
    let root = tempfile::tempdir().unwrap();
    assert!(collect_linux_identity(root.path()).is_err());
    net(root.path(), "eno1", "10:20:30:40:50:61", true, true);
    fact(root.path(), "block/loop0/device/serial", "ignored");
    fact(
        root.path(),
        "block/nvme0n1/device/serial",
        " D I S K - EXAMPLE ",
    );
    fact(
        root.path(),
        "class/dmi/id/board_serial",
        "To Be Filled By O.E.M.",
    );
    fact(root.path(), "class/dmi/id/product_serial", " BOARD \t 123 ");
    fact(root.path(), "class/dmi/id/sys_vendor", "Example");
    fact(root.path(), "class/dmi/id/product_name", "PC");
    let f = collect_linux_identity(root.path()).unwrap();
    assert_eq!(f.derived_persistent_id(), fixture()["id"]);
    let service = IdentityService::new(Arc::new(
        StateStore::open(root.path().join("state")).unwrap(),
    ));
    assert!(
        service
            .resolve(&f)
            .unwrap()
            .client_sign()
            .unwrap()
            .contains("@DISK-EXAMPLE@BOARD123@")
    );
    fact(root.path(), "class/dmi/id/product_name", "🎵");
    assert!(collect_linux_identity(root.path()).is_err());
    fact(root.path(), "class/dmi/id/product_name", "PC");
    fact(root.path(), "class/dmi/id/chassis_type", "10");
    assert_ne!(
        collect_linux_identity(root.path())
            .unwrap()
            .derived_persistent_id(),
        fixture()["id"]
    );
}
#[test]
fn persisted_id_survives_real_reopen_hardware_change_and_legacy_python_schema() {
    let root = tempfile::tempdir().unwrap();
    let a = facts("10:20:30:40:50:61");
    let b = facts("10:20:30:40:50:62");
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let service = IdentityService::new(store.clone());
    let first = service.resolve(&a).unwrap().persistent_id().to_owned();
    drop(service);
    drop(store);
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let service = IdentityService::new(store.clone());
    let reopened = service.resolve(&b).unwrap();
    assert_eq!(reopened.persistent_id(), first);
    assert_ne!(b.derived_persistent_id(), first);
    assert!(
        reopened
            .client_sign()
            .unwrap()
            .starts_with("10:20:30:40:50:62@")
    );
    let legacy = "a".repeat(64);
    store
        .put(
            "device",
            "identity",
            "client_sign",
            &json!({"persistent_id":legacy,"creation_source":{"platform":"legacy-python"}}),
        )
        .unwrap();
    assert_eq!(service.resolve(&a).unwrap().persistent_id(), legacy);
    assert_eq!(
        store
            .get("device", "identity", "client_sign")
            .unwrap()
            .unwrap()["creation_source"]["platform"],
        "legacy-python"
    );
}
#[test]
fn independent_database_connections_race_to_one_persistent_identifier() {
    let root = tempfile::tempdir().unwrap();
    let a = IdentityService::new(Arc::new(StateStore::open(root.path()).unwrap()));
    let b = IdentityService::new(Arc::new(StateStore::open(root.path()).unwrap()));
    let barrier = Barrier::new(2);
    std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            barrier.wait();
            a.resolve(&facts("10:20:30:40:50:61"))
                .unwrap()
                .persistent_id()
                .to_owned()
        });
        let two = scope.spawn(|| {
            barrier.wait();
            b.resolve(&facts("10:20:30:40:50:62"))
                .unwrap()
                .persistent_id()
                .to_owned()
        });
        assert_eq!(one.join().unwrap(), two.join().unwrap());
    });
}
#[test]
fn failed_observation_transaction_and_corrupt_existing_identity_never_rotate() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let service = IdentityService::new(store.clone());
    let db = rusqlite::Connection::open(root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_observation BEFORE INSERT ON state WHEN NEW.kind='identity' AND NEW.key='last_observed' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(service.resolve(&facts("10:20:30:40:50:61")).is_err());
    assert!(store.values("device", "identity").unwrap().is_empty());
    db.execute_batch("DROP TRIGGER reject_observation;")
        .unwrap();
    store
        .put(
            "device",
            "identity",
            "client_sign",
            &json!({"persistent_id":"corrupt"}),
        )
        .unwrap();
    assert!(matches!(
        service.resolve(&facts("10:20:30:40:50:61")),
        Err(Error::State(_))
    ));
    assert_eq!(
        store
            .get("device", "identity", "client_sign")
            .unwrap()
            .unwrap()["persistent_id"],
        "corrupt"
    );
    assert!(
        store
            .get("device", "identity", "last_observed")
            .unwrap()
            .is_none()
    );
}
#[test]
fn explicit_container_import_export_preserves_credentials_and_refuses_conflicting_identity() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let service = IdentityService::new(store.clone());
    let credentials = json!({"device_id":"separate-profile-id","cookies":["synthetic-cookie"]});
    store
        .put("transport", "credentials", "session", &credentials)
        .unwrap();
    assert!(service.export_primary().is_err());
    let f = fixture();
    service.import_primary(&bytes(&f["container"])).unwrap();
    assert_eq!(
        service.export_primary().unwrap(),
        bytes(&f["canonicalContainer"])
    );
    service.import_primary(&bytes(&f["container"])).unwrap();
    let other = encode_container(&BTreeMap::from([(
        ID_RECORD_KEY,
        encode_record(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap(),
    )]))
    .unwrap();
    assert!(service.import_primary(&other).is_err());
    assert_eq!(
        store
            .get("transport", "credentials", "session")
            .unwrap()
            .unwrap(),
        credentials
    );
    let identity = service.resolve(&facts("10:20:30:40:50:62")).unwrap();
    let profile = identity
        .profile(ClientProfile {
            user_agent: "unchanged UA".into(),
            os_version: "Linux".into(),
            client_sign: None,
        })
        .unwrap();
    assert_eq!(profile.user_agent, "unchanged UA");
    assert_eq!(profile.os_version, "Linux");
    assert_eq!(identity.persistent_id(), f["id"]);
    assert!(
        !serde_json::to_string(identity.summary())
            .unwrap()
            .contains(identity.persistent_id())
    );
}
