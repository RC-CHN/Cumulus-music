use cumulus_core::{
    Error,
    account::credentials::Credentials,
    protocol::http::{ApiOrigin, WireRoute, select_route, validate_path},
    storage::StateStore,
};

#[test]
fn official_pc_playlist_policy_promotes_only_the_reviewed_workflow() {
    use cumulus_core::protocol::http::{RoutingPolicy, select_route_with_policy};
    for path in [
        "/api/playlist/create",
        "/api/v1/playlist/manipulate/tracks",
        "/api/playlist/delete",
        "/api/playlist/order/update",
    ] {
        assert_eq!(
            select_route_with_policy(
                ApiOrigin::Pc,
                path,
                &json!({}),
                RoutingPolicy::OfficialPcPlaylists
            )
            .unwrap(),
            WireRoute::Eapi
        );
    }
    for path in [
        "/api/playlist/update/name",
        "/api/playlist/desc/update",
        "/api/playlist/subscribe",
        "/api/playlist/unsubscribe",
        "/api/user/setting/update",
    ] {
        assert_eq!(
            select_route_with_policy(
                ApiOrigin::Pc,
                path,
                &json!({}),
                RoutingPolicy::OfficialPcPlaylists
            )
            .unwrap(),
            WireRoute::Weapi
        );
    }
}
use serde_json::{Value, json};
use std::sync::Arc;
use url::Url;

fn url(path: &str) -> Url {
    ApiOrigin::Pc.request_url(WireRoute::Eapi, path).unwrap()
}

#[test]
fn session_cookies_and_refresh_credentials_survive_restart_without_scope_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    let endpoint = url("/api/login/qrcode/client/login");
    let snapshot = credentials.request_credentials(&endpoint).unwrap();
    credentials
        .record_response(
            &endpoint,
            &[
                "MUSIC_U=synthetic-session; Path=/; Secure; HttpOnly".into(),
                "parent=server-domain-cookie; Domain=music.163.com; Path=/; Secure".into(),
                "scoped=login-only; Path=/eapi/login; Secure".into(),
            ],
            Some("synthetic-refresh"),
            &snapshot.revision,
        )
        .unwrap();
    let saved = store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    let auth = saved["cookies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["raw_cookie"].as_str().unwrap().starts_with("MUSIC_U="))
        .unwrap();
    assert_eq!(
        auth["domain"],
        json!({"HostOnly":"interfacepc.music.163.com"})
    );
    drop(credentials);
    drop(store);
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    assert_eq!(credentials.device_id().unwrap(), snapshot.device_id);
    let login = credentials.login_credentials().unwrap();
    assert_eq!(login.token.as_deref(), Some("synthetic-session"));
    assert_eq!(login.refresh_token.as_deref(), Some("synthetic-refresh"));
    let read = credentials
        .cookies_for(&url("/api/w/nuser/account/get"))
        .unwrap();
    assert!(!read.iter().any(|(name, _)| name == "scoped"));
    assert!(read.iter().any(|(name, _)| name == "parent"));
    assert!(
        credentials
            .cookies_for(&url("/api/login/token/refresh/v1"))
            .unwrap()
            .iter()
            .any(|(name, _)| name == "scoped")
    );
    assert!(
        !credentials
            .cookies_for(&url("/api/loginish"))
            .unwrap()
            .iter()
            .any(|(name, _)| name == "scoped")
    );
    assert!(Credentials::open(store, ApiOrigin::Interface).is_err());
}

#[test]
fn host_only_parent_cookie_is_not_sent_to_pc_and_external_targets_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    drop(credentials);
    let mut raw = store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    let parent = cookie_store::Cookie::parse(
        "parent=other-origin; Path=/; Secure",
        &Url::parse("https://music.163.com/").unwrap(),
    )
    .unwrap()
    .into_owned();
    raw["cookies"] = json!([parent]);
    store
        .put("transport", "credentials", "session", &raw)
        .unwrap();
    let credentials = Credentials::open(store, ApiOrigin::Pc).unwrap();
    assert!(
        credentials
            .cookies_for(&url("/api/test"))
            .unwrap()
            .is_empty()
    );
    for target in [
        "https://music.163.com/eapi/test",
        "https://interface.music.163.com/eapi/test",
        "https://interfacepc.music.163.com.attacker.invalid/eapi/test",
        "http://interfacepc.music.163.com/eapi/test",
        "https://user:password@interfacepc.music.163.com/eapi/test",
        "https://interfacepc.music.163.com:8443/eapi/test",
    ] {
        assert!(
            credentials
                .cookies_for(&Url::parse(target).unwrap())
                .is_err()
        );
    }
}

#[test]
fn logout_rejects_late_cookie_and_token_resurrection_in_same_or_another_instance() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    let endpoint = url("/api/login/qrcode/client/login");
    let old = credentials.request_credentials(&endpoint).unwrap();
    credentials.clear().unwrap();
    assert!(matches!(
        credentials.record_response(
            &endpoint,
            &["MUSIC_U=late; Path=/; Secure".into()],
            Some("late-refresh"),
            &old.revision
        ),
        Err(Error::CredentialChanged)
    ));
    assert!(matches!(
        credentials.record_response(&endpoint, &[], None, &old.revision),
        Err(Error::CredentialChanged)
    ));
    assert!(credentials.login_credentials().unwrap().token.is_none());
    let other = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    let before = credentials.request_credentials(&endpoint).unwrap();
    other.clear().unwrap();
    assert!(matches!(
        credentials.cookies_for(&endpoint),
        Err(Error::CredentialChanged)
    ));
    assert!(matches!(
        credentials.record_response(
            &endpoint,
            &["MUSIC_U=late; Path=/; Secure".into()],
            None,
            &before.revision
        ),
        Err(Error::CredentialChanged)
    ));
    assert!(
        Credentials::open(store, ApiOrigin::Pc)
            .unwrap()
            .login_credentials()
            .unwrap()
            .token
            .is_none()
    );
}

#[test]
fn ticket_is_host_only_and_cookie_deletion_is_persisted() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    for ticket in ["", "bad;injected=1", "bad\r\nCookie: x", "bad\0"] {
        assert!(credentials.set_login_ticket(ticket).is_err());
    }
    credentials.set_login_ticket("synthetic-ticket").unwrap();
    let saved = store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    assert_eq!(
        saved["cookies"][0]["domain"],
        json!({"HostOnly":"interfacepc.music.163.com"})
    );
    let endpoint = url("/api/test");
    let snapshot = credentials.request_credentials(&endpoint).unwrap();
    assert!(snapshot.cookies.iter().any(|(name, _)| name == "MUSIC_TU"));
    credentials
        .record_response(
            &endpoint,
            &["MUSIC_TU=deleted; Path=/; Secure; Max-Age=0".into()],
            None,
            &snapshot.revision,
        )
        .unwrap();
    assert!(credentials.cookies_for(&endpoint).unwrap().is_empty());
    drop(credentials);
    assert!(
        Credentials::open(store, ApiOrigin::Pc)
            .unwrap()
            .cookies_for(&endpoint)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn failed_credential_commit_leaves_memory_and_disk_at_previous_revision() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
    let before = credentials.login_credentials().unwrap();
    let db = rusqlite::Connection::open(tmp.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_write BEFORE INSERT ON state BEGIN SELECT RAISE(ABORT,'synthetic fault'); END").unwrap();
    assert!(matches!(
        credentials.set_login_ticket("synthetic"),
        Err(Error::Storage(_))
    ));
    let after = credentials.login_credentials().unwrap();
    assert_eq!(before.revision, after.revision);
    assert!(
        credentials
            .cookies_for(&url("/api/test"))
            .unwrap()
            .is_empty()
    );
    db.execute_batch("DROP TRIGGER reject_write").unwrap();
    credentials.set_login_ticket("synthetic").unwrap();
    assert_ne!(
        credentials.login_credentials().unwrap().revision,
        after.revision
    );
}

#[test]
fn longest_matching_cookie_path_precedes_a_same_name_root_cookie() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let credentials = Credentials::open(store, ApiOrigin::Pc).unwrap();
    let target = url("/api/login/token/refresh/v1");
    let before = credentials.request_credentials(&target).unwrap();
    credentials
        .record_response(
            &target,
            &[
                "MUSIC_U=root; Path=/; Secure".into(),
                "MUSIC_U=login; Path=/eapi/login; Secure".into(),
            ],
            None,
            &before.revision,
        )
        .unwrap();
    let values: Vec<_> = credentials
        .cookies_for(&target)
        .unwrap()
        .into_iter()
        .filter(|(name, _)| name == "MUSIC_U")
        .map(|(_, value)| value)
        .collect();
    assert_eq!(values, vec!["login", "root"]);
    assert_eq!(
        credentials.login_credentials().unwrap().token.as_deref(),
        Some("login")
    );
    assert_eq!(
        credentials.cookies_for(&url("/api/test")).unwrap(),
        vec![("MUSIC_U".into(), "root".into())]
    );
}

#[test]
fn rotation_receipt_requires_an_unexpired_effective_music_u_in_the_same_response() {
    let tmp = tempfile::tempdir().unwrap();
    let credentials = Credentials::open(
        Arc::new(StateStore::open(tmp.path()).unwrap()),
        ApiOrigin::Pc,
    )
    .unwrap();
    let endpoint = url("/api/login/token/refresh/v1");
    let initial = credentials.request_credentials(&endpoint).unwrap();
    credentials
        .record_response(
            &endpoint,
            &[
                "MUSIC_U=root; Path=/; Secure".into(),
                "MUSIC_U=scoped; Path=/eapi/login; Secure".into(),
            ],
            Some("old-refresh"),
            &initial.revision,
        )
        .unwrap();
    let original = credentials.login_credentials().unwrap().rotation;
    assert!(original.is_some());
    let before = credentials.request_credentials(&endpoint).unwrap();
    credentials
        .record_response(
            &endpoint,
            &["MUSIC_U=changed-root; Path=/; Secure".into()],
            Some("new-refresh"),
            &before.revision,
        )
        .unwrap();
    assert_eq!(credentials.login_credentials().unwrap().rotation, original);
    let before = credentials.request_credentials(&endpoint).unwrap();
    credentials
        .record_response(
            &endpoint,
            &["MUSIC_U=deleted; Path=/eapi/login; Max-Age=0".into()],
            Some("another-refresh"),
            &before.revision,
        )
        .unwrap();
    let after = credentials.login_credentials().unwrap();
    assert_eq!(after.token.as_deref(), Some("changed-root"));
    assert_eq!(after.rotation, original);
}

#[test]
fn routing_is_fixed_before_send_and_only_the_exact_pc_metadata_batch_uses_weapi() {
    let writes = [
        "/api/playlist/create",
        "/api/playlist/update/name",
        "/api/playlist/desc/update",
        "/api/v1/playlist/manipulate/tracks",
        "/api/playlist/delete",
        "/api/user/setting/update",
    ];
    for path in writes {
        assert_eq!(
            select_route(ApiOrigin::Pc, path, &json!({})).unwrap(),
            WireRoute::Weapi
        );
        assert_eq!(
            select_route(ApiOrigin::Interface, path, &json!({})).unwrap(),
            WireRoute::Eapi
        );
    }
    let mut payload = json!({"/api/playlist/update/name":"{}","/api/playlist/tags/update":"{}","/api/playlist/desc/update":"{}"});
    assert_eq!(
        select_route(ApiOrigin::Pc, "/api/batch", &payload).unwrap(),
        WireRoute::Weapi
    );
    payload["/api/another/business"] = json!("{}");
    assert_eq!(
        select_route(ApiOrigin::Pc, "/api/batch", &payload).unwrap(),
        WireRoute::Eapi
    );
    assert_eq!(
        select_route(ApiOrigin::Pc, "/api/song/like", &json!({})).unwrap(),
        WireRoute::Eapi
    );
    for path in [
        "https://music.163.com/api/test",
        "/api/../test",
        "/api//test",
        "/api/%2e%2e/test",
        "/api/test?q=1",
        "/api/test#fragment",
        "/api/a\\b",
        "/api/",
    ] {
        assert!(validate_path(path).is_err());
    }
    assert!(select_route(ApiOrigin::Pc, "/api/test", &Value::Null).is_err());
}
