use cumulus_core::{
    account::{
        credential_import::{LegacySession, parse_netscape},
        credentials::Credentials,
    },
    protocol::http::{ApiOrigin, WireRoute},
    storage::StateStore,
};
use serde_json::json;
use std::{fs, path::Path, sync::Arc};

const COOKIES: &str = "# Netscape HTTP Cookie File\n\
#HttpOnly_interfacepc.music.163.com\tFALSE\t/\tTRUE\t\tMUSIC_U\tsynthetic-session\n\
.music.163.com\tTRUE\t/\tTRUE\t4102444800\tparent\tparent-value\n\
music.163.com\tFALSE\t/\tTRUE\t\twrong_host\tnot-for-pc\n\
interface.music.163.com\tFALSE\t/\tTRUE\t\twrong_interface\tnot-for-pc\n\
interfacepc.music.163.com\tFALSE\t/eapi/login\tTRUE\t\tscoped\tlogin-only\n\
interfacepc.music.163.com\tFALSE\t/\tFALSE\t0\texpired\told-value\n";

fn fixture(dir: &Path) {
    fs::write(
        dir.join("device.json"),
        r#"{"deviceId":"synthetic-device"}"#,
    )
    .unwrap();
    fs::write(dir.join("cookies.txt"), COOKIES).unwrap();
    fs::write(
        dir.join("refresh-token.json"),
        json!({"host":ApiOrigin::Pc.as_str(),"refresh_token":"synthetic-refresh"}).to_string(),
    )
    .unwrap();
}

#[test]
fn explicit_import_preserves_scope_flags_expiry_device_and_source_files() {
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    let before: Vec<_> = ["device.json", "cookies.txt", "refresh-token.json"]
        .iter()
        .map(|p| fs::read(source.path().join(p)).unwrap())
        .collect();
    let destination = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(destination.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    credentials
        .import_legacy(LegacySession::read(source.path(), ApiOrigin::Pc).unwrap())
        .unwrap();
    assert_eq!(credentials.device_id().unwrap(), "synthetic-device");
    let login = credentials.login_credentials().unwrap();
    assert_eq!(login.token.as_deref(), Some("synthetic-session"));
    assert_eq!(login.refresh_token.as_deref(), Some("synthetic-refresh"));
    let url = ApiOrigin::Pc
        .request_url(WireRoute::Eapi, "/api/w/nuser/account/get")
        .unwrap();
    let cookies = credentials.cookies_for(&url).unwrap();
    assert_eq!(cookies.len(), 2);
    assert!(cookies.iter().any(|(name, _)| name == "parent"));
    let raw = store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    let cookies = raw["cookies"].as_array().unwrap();
    let session = cookies
        .iter()
        .find(|c| c["raw_cookie"].as_str().unwrap().starts_with("MUSIC_U="))
        .unwrap();
    assert_eq!(
        session["domain"],
        json!({"HostOnly":"interfacepc.music.163.com"})
    );
    assert!(session["raw_cookie"].as_str().unwrap().contains("HttpOnly"));
    assert!(session["raw_cookie"].as_str().unwrap().contains("Secure"));
    assert_eq!(session["expires"], json!("SessionEnd"));
    drop(credentials);
    let reopened = Credentials::open(store, ApiOrigin::Pc).unwrap();
    let login_url = ApiOrigin::Pc
        .request_url(WireRoute::Eapi, "/api/login/token/refresh/v1")
        .unwrap();
    assert_eq!(reopened.cookies_for(&login_url).unwrap().len(), 3);
    let after: Vec<_> = ["device.json", "cookies.txt", "refresh-token.json"]
        .iter()
        .map(|p| fs::read(source.path().join(p)).unwrap())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn mismatched_origin_or_nonempty_destination_does_not_replace_credentials() {
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    assert!(LegacySession::read(source.path(), ApiOrigin::Interface).is_err());
    let destination = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(destination.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Interface).unwrap();
    let before = store.get("transport", "credentials", "session").unwrap();
    assert!(
        credentials
            .import_legacy(LegacySession::read(source.path(), ApiOrigin::Pc).unwrap())
            .is_err()
    );
    assert_eq!(
        store.get("transport", "credentials", "session").unwrap(),
        before
    );
    let destination = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(destination.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    credentials.set_login_ticket("pending-attempt").unwrap();
    let before = store.get("transport", "credentials", "session").unwrap();
    assert!(
        credentials
            .import_legacy(LegacySession::read(source.path(), ApiOrigin::Pc).unwrap())
            .is_err()
    );
    assert_eq!(
        store.get("transport", "credentials", "session").unwrap(),
        before
    );
}

#[test]
fn malformed_credential_files_are_rejected_and_missing_refresh_is_allowed() {
    for line in [
        "interfacepc.music.163.com\tMAYBE\t/\tTRUE\t\tn\tv",
        ".music.163.com\tFALSE\t/\tTRUE\t\tn\tv",
        "interfacepc.music.163.com\tFALSE\t/\tTRUE\tbad\tn\tv",
        "interfacepc.music.163.com\tFALSE\t/\tTRUE\t\tn\tv; injected=1",
        "interfacepc.music.163.com\tFALSE\tbad-path\tTRUE\t\tn\tv",
        "interfacepc.music.163.com\tFALSE\t/\tTRUE\t\tn",
    ] {
        assert!(
            parse_netscape(
                &format!("# Netscape HTTP Cookie File\n{line}\n"),
                ApiOrigin::Pc
            )
            .is_err()
        );
    }
    assert!(parse_netscape("no header", ApiOrigin::Pc).is_err());
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    fs::remove_file(source.path().join("refresh-token.json")).unwrap();
    assert!(LegacySession::read(source.path(), ApiOrigin::Pc).is_ok());
    fs::write(source.path().join("device.json"), "x".repeat(32769)).unwrap();
    assert!(LegacySession::read(source.path(), ApiOrigin::Pc).is_err());
}

#[cfg(unix)]
#[test]
fn final_symlink_is_not_accepted_as_a_credential_file() {
    let source = tempfile::tempdir().unwrap();
    fixture(source.path());
    fs::rename(
        source.path().join("device.json"),
        source.path().join("real-device.json"),
    )
    .unwrap();
    std::os::unix::fs::symlink("real-device.json", source.path().join("device.json")).unwrap();
    assert!(LegacySession::read(source.path(), ApiOrigin::Pc).is_err());
}
