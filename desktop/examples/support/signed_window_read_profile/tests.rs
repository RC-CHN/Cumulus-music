use super::*;
use cumulus_core::{account::profiles::ProfileRegistry, identity::IdentitySource};
use rusqlite::Connection;
use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
};

const UID: &str = "42";
const SESSION_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ACCOUNT: &str = "/api/w/nuser/account/get";

fn facts() -> Result<HardwareFacts> {
    HardwareFacts::from_encoded(
        [
            "synthetic MAC".into(),
            "synthetic disk".into(),
            "synthetic board".into(),
        ],
        b"synthetic hardware".to_vec(),
        b"synthetic host".to_vec(),
        IdentitySource {
            platform: "synthetic".into(),
            interface: "".into(),
            disk_device: "".into(),
            host_description_available: true,
        },
    )
}

struct Fixture {
    _outer: TempDir,
    source: PathBuf,
    private: PathBuf,
    selected: PathBuf,
}

impl Fixture {
    fn new(identity: bool) -> Self {
        let outer = tempfile::tempdir().unwrap();
        let source = outer.path().join("source");
        let private = outer.path().join("scratch");
        cumulus_core::storage::private_directory_builder()
            .create(&private)
            .unwrap();
        let registry = ProfileRegistry::open(&source, ApiOrigin::Pc).unwrap();
        let store = registry.store();
        store
            .put(
                TRANSPORT,
                PROFILE,
                "active",
                &json!({"session_id":SESSION_ID,"revision":"b".repeat(32),"auto_login":true}),
            )
            .unwrap();
        store.put(TRANSPORT, SAVED, UID, &json!({"uid":UID,"session_id":SESSION_ID,"nickname":"only in source","avatar_url":null,"auto_login":true})).unwrap();
        store.put(TRANSPORT, SAVED, "43", &json!({"uid":"43","session_id":"c".repeat(32),"nickname":"other account","avatar_url":null,"auto_login":true})).unwrap();
        store.put(TRANSPORT, PC_CONFIG, "pc", &json!({
            "origin":ApiOrigin::Pc,"version":7,"bucket":"","generation":1,
            "source_url":"https://m1.music.126.net/obj/synthetic.data",
            "data":{"yidun#paths":{"whiteList":[{"businessId":"synthetic","list":["/api/song/like/get"]}]}},
        })).unwrap();
        if identity {
            store.put(DEVICE, IDENTITY, CLIENT_SIGN, &json!({"persistent_id":"d".repeat(64),"creation_source":{"platform":"source only"}})).unwrap();
        }
        store
            .put(
                DEVICE,
                IDENTITY,
                "last_observed",
                &json!({"source":"must not persist"}),
            )
            .unwrap();
        store
            .put(
                UID,
                "download_tasks_v1",
                "foreign-fixture-row",
                &json!({"song_id":"77"}),
            )
            .unwrap();
        drop((store, registry));
        let selected = source.join("sessions").join(SESSION_ID);
        let credentials = Arc::new(StateStore::open(&selected).unwrap());
        let transport = Credentials::open(credentials.clone(), ApiOrigin::Pc).unwrap();
        let account = ApiOrigin::Pc.request_url(WireRoute::Eapi, ACCOUNT).unwrap();
        let revision = transport.request_credentials(&account).unwrap().revision;
        transport
            .record_response(
                &account,
                &["MUSIC_U=synthetic-proof-only; Path=/eapi; Secure; HttpOnly".into()],
                None,
                &revision,
            )
            .unwrap();
        credentials
            .put(
                UID,
                "offline_music_v1",
                "foreign-fixture-row",
                &json!({"x":"not copied"}),
            )
            .unwrap();
        drop((transport, credentials));
        Self {
            _outer: outer,
            source,
            private,
            selected,
        }
    }

    fn prepare(&self, mode: IdentityMode) -> Result<PreparedProfile> {
        prepare_with(&self.source, &self.private, UID, mode, facts)
    }

    fn bytes(&self) -> [Vec<u8>; 2] {
        [&self.source, &self.selected].map(|path| fs::read(path.join("core.sqlite3")).unwrap())
    }

    fn empty_scratch(&self) -> bool {
        self.private.read_dir().unwrap().next().is_none()
    }
}

#[tokio::test]
async fn copies_only_selected_record_and_config_without_exposing_identity_facts() {
    let fixture = Fixture::new(true);
    let before = fixture.bytes();
    let profile = fixture.prepare(IdentityMode::DeriveIfMissing).unwrap();
    let root = profile.state_dir().parent().unwrap().to_path_buf();
    assert_eq!(root.metadata().unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        profile
            .credential_dir()
            .join("core.sqlite3")
            .metadata()
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!profile.identity_was_derived());
    assert!(
        profile
            .native_profile()
            .client_sign
            .as_ref()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(profile.config_snapshot().unwrap().version, 7);
    assert_eq!(
        profile
            .config_dir()
            .join("core.sqlite3")
            .metadata()
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    let restored = profile.credentials();
    let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, ACCOUNT).unwrap();
    assert_eq!(
        restored.cookies_for(&target).unwrap(),
        [("MUSIC_U".into(), "synthetic-proof-only".into())]
    );
    let outside_scope = ApiOrigin::Pc
        .request_url(WireRoute::Weapi, ACCOUNT)
        .unwrap();
    assert!(restored.cookies_for(&outside_scope).unwrap().is_empty());
    assert!(
        restored
            .cookies_for(
                &ApiOrigin::Interface
                    .request_url(WireRoute::Eapi, ACCOUNT)
                    .unwrap()
            )
            .is_err()
    );
    drop(restored);
    let credential = Connection::open(profile.credential_dir().join("core.sqlite3")).unwrap();
    let rows: i64 = credential
        .query_row("SELECT count(*) FROM state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);
    drop(credential);
    let application = Connection::open(profile.state_dir().join("core.sqlite3")).unwrap();
    let rows: i64 = application
        .query_row("SELECT count(*) FROM state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    drop(application);
    assert_eq!(root.read_dir().unwrap().count(), 3);
    let context = profile.request_context();
    assert!(context.prepare(ACCOUNT, &json!({})).await.is_ok());
    assert!(
        context
            .prepare("/api/song/like/get", &json!({}))
            .await
            .is_err()
    );
    assert!(
        ConfigBackend::get(
            &DenyAuxiliary,
            &ConfigUrl::parse("https://m1.music.126.net/obj/synthetic.data").unwrap()
        )
        .await
        .is_err()
    );
    drop(context);
    profile.close().unwrap();
    assert!(!root.exists() && fixture.empty_scratch());
    assert_eq!(before, fixture.bytes());
}

#[test]
fn derivation_requires_an_explicit_missing_identity_mode_and_never_replaces_invalid_saved_value() {
    let fixture = Fixture::new(false);
    assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
    assert!(fixture.empty_scratch());
    let proof = fixture.prepare(IdentityMode::DeriveIfMissing).unwrap();
    assert!(proof.identity_was_derived());
    drop(proof);
    assert!(fixture.empty_scratch());
    StateStore::open(&fixture.source)
        .unwrap()
        .put(
            DEVICE,
            IDENTITY,
            CLIENT_SIGN,
            &json!({"persistent_id":"invalid"}),
        )
        .unwrap();
    assert!(fixture.prepare(IdentityMode::DeriveIfMissing).is_err());
    assert!(fixture.empty_scratch());
}

#[test]
fn rejects_mismatch_duplicate_binding_and_pending_removal_or_auth_cleanup() {
    let fixture = Fixture::new(true);
    assert!(
        prepare(
            &fixture.source,
            &fixture.private,
            "43",
            IdentityMode::RequireSaved
        )
        .is_err()
    );
    let registry = StateStore::open(&fixture.source).unwrap();
    registry.put(TRANSPORT, SAVED, "43", &json!({"uid":"43","session_id":SESSION_ID,"nickname":"other account","avatar_url":null,"auto_login":true})).unwrap();
    drop(registry);
    assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
    let registry = StateStore::open(&fixture.source).unwrap();
    registry.delete(TRANSPORT, SAVED, "43").unwrap();
    registry
        .put(TRANSPORT, REMOVAL, SESSION_ID, &json!({"complete":false}))
        .unwrap();
    drop(registry);
    assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
    StateStore::open(&fixture.source)
        .unwrap()
        .delete(TRANSPORT, REMOVAL, SESSION_ID)
        .unwrap();
    for (kind, pending) in [("jsk", json!(true)), ("legacy-periodic", json!("pending"))] {
        StateStore::open(&fixture.source)
            .unwrap()
            .put(UID, "login_refresh", kind, &json!({"pending":pending}))
            .unwrap();
        assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
        StateStore::open(&fixture.source)
            .unwrap()
            .delete(UID, "login_refresh", kind)
            .unwrap();
    }
    let credentials = StateStore::open(&fixture.selected).unwrap();
    let original = credentials
        .get(TRANSPORT, CREDENTIAL, SESSION)
        .unwrap()
        .unwrap();
    for field in ["login_notice", "queue_reset"] {
        let mut record = original.clone();
        record[field] = json!({"fixture":"pending"});
        credentials
            .put(TRANSPORT, CREDENTIAL, SESSION, &record)
            .unwrap();
        assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
    }
    assert!(fixture.empty_scratch());
}

#[test]
fn refuses_symlink_hardlink_active_sidecars_and_world_accessible_source() {
    let fixture = Fixture::new(true);
    let link = fixture.source.parent().unwrap().join("redirected");
    symlink(&fixture.source, &link).unwrap();
    assert!(
        prepare_with(
            &link,
            &fixture.private,
            UID,
            IdentityMode::RequireSaved,
            facts
        )
        .is_err()
    );
    let second_path = fixture.private.join("second-link");
    fs::hard_link(fixture.selected.join("core.sqlite3"), &second_path).unwrap();
    assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
    fs::remove_file(second_path).unwrap();
    let sidecar = fixture.selected.join("core.sqlite3-wal");
    fs::write(&sidecar, b"not an admissible stopped source").unwrap();
    assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
    fs::remove_file(sidecar).unwrap();
    fs::set_permissions(
        fixture.selected.join("core.sqlite3"),
        Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(fixture.prepare(IdentityMode::RequireSaved).is_err());
}

#[test]
fn closed_checkpointed_wal_header_is_normalized_only_in_memory() {
    let fixture = Fixture::new(true);
    let file = fixture.source.join("core.sqlite3");
    let database = Connection::open(&file).unwrap();
    database
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(database);
    assert!(!fixture.source.join("core.sqlite3-wal").exists());
    let before = fixture.bytes();
    assert_eq!(&before[0][18..20], &[2, 2]);
    let proof = fixture.prepare(IdentityMode::RequireSaved).unwrap();
    proof.close().unwrap();
    assert_eq!(before, fixture.bytes());
}
