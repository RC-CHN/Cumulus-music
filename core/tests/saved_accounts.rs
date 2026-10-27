mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{LoginCommand, LoginMode},
    },
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions},
    storage::StateStore,
};
use serde_json::json;
use std::sync::Arc;
use support::http::{HttpResponse, ScriptHttp, reply};

fn identity(uid: u64) -> HttpResponse {
    json!({"code":200,"profile":{"userId":uid,"nickname":format!("account{uid}")}}).into()
}
fn login(uid: u64) -> [HttpResponse; 2] {
    let mut r = reply(json!({"code":200}));
    r.set_cookies = vec![format!("MUSIC_U=saved-fixture-{uid}; Path=/; Secure")];
    r.refresh_token = Some(format!("refresh-fixture-{uid}"));
    [HttpResponse::Ready(Ok(r)), identity(uid)]
}
struct World {
    root: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    http: Arc<ScriptHttp>,
    client: MusicClient,
}
impl World {
    async fn new(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let http = Arc::new(ScriptHttp::new([identity(1)].into_iter().chain(replies)));
        let credentials = Arc::new(
            Credentials::open(
                Arc::new(StateStore::open(root.path().join("guest")).unwrap()),
                ApiOrigin::Pc,
            )
            .unwrap(),
        );
        let api = Arc::new(
            MusicHttpClient::with_backend(
                credentials,
                ClientProfile::default(),
                RequestOptions::default(),
                None,
                http.clone(),
            )
            .unwrap(),
        );
        let session = Arc::new(AccountSession::new(api, Some(registry.store())));
        let client = MusicClient::with_session(session.clone(), registry.clone())
            .with_login_providers(Some(http.clone()), None);
        client.restore_account().await.unwrap();
        Self {
            root,
            registry,
            session,
            http,
            client,
        }
    }
    fn epoch(&self) -> String {
        self.client.account().unwrap().epoch
    }
    async fn login(&self, remember: bool) -> String {
        let flow = self
            .client
            .begin_login(&self.epoch(), LoginMode::Email, None)
            .await
            .unwrap();
        self.client
            .login_command(
                &flow.operation_id,
                LoginCommand::Email {
                    address: "fixture@example.test".into(),
                    password: "fixture-password".into(),
                    remember,
                },
            )
            .await
            .unwrap();
        self.registry.selection().unwrap().unwrap().session_id
    }
    fn credentials(&self, id: &str) -> Arc<StateStore> {
        Arc::new(StateStore::open(self.root.path().join("sessions").join(id)).unwrap())
    }
}

#[tokio::test]
async fn removal_clears_only_selected_saved_session_blocks_old_candidate_and_allows_new_login() {
    let w = World::new(login(2).into_iter().chain(login(3)).chain(login(2))).await;
    let id = w.login(true).await;
    let keep = w.login(true).await;
    let stale = w
        .registry
        .saved_candidate_with_backend(
            &id,
            Default::default(),
            Default::default(),
            None,
            w.http.clone(),
        )
        .unwrap();
    let creds = stale.credentials();
    let before = creds.request_credentials(&ApiOrigin::Pc.url()).unwrap();
    let keep_before = w
        .credentials(&keep)
        .get("transport", "credentials", "session")
        .unwrap();
    w.registry
        .store()
        .put("2", "preferences", "keep", &json!({"value":1}))
        .unwrap();
    let calls = w.http.calls().len();
    w.client
        .remove_saved_account(&w.epoch(), &id)
        .await
        .unwrap();
    w.client
        .remove_saved_account(&w.epoch(), &id)
        .await
        .unwrap();
    assert_eq!(w.http.calls().len(), calls);
    assert_eq!(w.registry.saved().unwrap().len(), 1);
    assert!(
        w.credentials(&id)
            .get("transport", "credentials", "session")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        w.credentials(&keep)
            .get("transport", "credentials", "session")
            .unwrap(),
        keep_before
    );
    assert_eq!(
        w.registry.store().get("2", "preferences", "keep").unwrap(),
        Some(json!({"value":1}))
    );
    assert!(
        stale
            .verify_and_adopt(&w.session, &w.session.token().unwrap())
            .await
            .is_err()
    );
    assert!(
        creds
            .record_response(
                &ApiOrigin::Pc.url(),
                &["MUSIC_U=late; Path=/".into()],
                None,
                &before.revision
            )
            .is_err()
    );
    assert_eq!(w.http.calls().len(), calls);
    assert_eq!(
        w.client.account().unwrap().nickname.as_deref(),
        Some("account3")
    );
    let replacement = w.login(true).await;
    assert_ne!(replacement, id);
    assert_eq!(w.registry.saved().unwrap().len(), 2);
}

#[tokio::test]
async fn current_account_and_stale_or_invalid_handles_are_rejected_without_modification() {
    let w = World::new(login(2)).await;
    let old_epoch = w.epoch();
    let id = w.login(true).await;
    let before = w
        .credentials(&id)
        .get("transport", "credentials", "session")
        .unwrap();
    assert!(matches!(
        w.client.remove_saved_account(&w.epoch(), &id).await,
        Err(Error::State("cannot remove current account"))
    ));
    assert!(matches!(
        w.client.remove_saved_account(&old_epoch, &id).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        w.client
            .remove_saved_account(&w.epoch(), "../outside")
            .await
            .is_err()
    );
    assert!(!w.registry.removal_pending(&id).unwrap());
    assert_eq!(
        w.credentials(&id)
            .get("transport", "credentials", "session")
            .unwrap(),
        before
    );
    assert_eq!(w.http.calls().len(), 3);
}

#[tokio::test]
async fn failed_credential_cleanup_remains_visible_and_recovers_after_restart() {
    let w = World::new(login(2).into_iter().chain(login(3))).await;
    let id = w.login(true).await;
    w.login(true).await;
    let path = w.credentials(&id).directory().join("core.sqlite3");
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("CREATE TRIGGER deny_cleanup BEFORE DELETE ON state WHEN OLD.kind='credentials' BEGIN SELECT RAISE(FAIL, 'fixture interrupted cleanup'); END;").unwrap();
    assert!(
        w.client
            .remove_saved_account(&w.epoch(), &id)
            .await
            .is_err()
    );
    let pending = w.client.saved_accounts(&w.epoch()).unwrap().value;
    assert!(pending.iter().find(|a| a.id == id).unwrap().removal_pending);
    assert!(
        w.client
            .begin_login(&w.epoch(), LoginMode::SavedAccount, Some(&id))
            .await
            .is_err()
    );
    let backend = Arc::new(ScriptHttp::new([identity(3)]));
    let reopened =
        MusicClient::open_with_http_backend(w.root.path(), None, backend.clone()).unwrap();
    let account = reopened.restore_account().await.unwrap();
    assert!(
        reopened
            .saved_accounts(&account.epoch)
            .unwrap()
            .value
            .iter()
            .find(|a| a.id == id)
            .unwrap()
            .removal_pending
    );
    db.execute_batch("DROP TRIGGER deny_cleanup;").unwrap();
    reopened
        .remove_saved_account(&account.epoch, &id)
        .await
        .unwrap();
    assert_eq!(
        reopened.saved_accounts(&account.epoch).unwrap().value.len(),
        1
    );
    assert_eq!(backend.calls().len(), 1);
}

#[tokio::test]
async fn failed_registry_finish_keeps_tombstone_and_retry_does_not_recreate_credentials() {
    let w = World::new(login(2).into_iter().chain(login(3))).await;
    let id = w.login(true).await;
    w.login(true).await;
    let db = rusqlite::Connection::open(w.root.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER deny_finish BEFORE DELETE ON state WHEN OLD.kind='saved-profiles-v1' BEGIN SELECT RAISE(FAIL, 'fixture interrupted finish'); END;").unwrap();
    assert!(
        w.client
            .remove_saved_account(&w.epoch(), &id)
            .await
            .is_err()
    );
    assert!(w.registry.removal_pending(&id).unwrap());
    assert!(
        w.credentials(&id)
            .get("transport", "credentials", "session")
            .unwrap()
            .is_none()
    );
    db.execute_batch("DROP TRIGGER deny_finish;").unwrap();
    w.client
        .remove_saved_account(&w.epoch(), &id)
        .await
        .unwrap();
    assert!(!w.registry.removal_pending(&id).unwrap());
    assert!(
        w.credentials(&id)
            .get("transport", "credentials", "session")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn signed_out_bound_profile_removal_preserves_snapshot_and_clears_startup_selection() {
    let w = World::new(login(2)).await;
    let id = w.login(true).await;
    w.client.logout_account(&w.epoch(), false).await.unwrap();
    let epoch = w.epoch();
    w.client.remove_saved_account(&epoch, &id).await.unwrap();
    assert!(!w.client.account().unwrap().can_sign_out);
    assert!(w.registry.selection().unwrap().is_none());
    assert!(w.client.saved_accounts(&epoch).unwrap().value.is_empty());
    assert_eq!(w.http.calls().len(), 3);
}

#[tokio::test]
async fn guest_can_remove_nonautomatic_selected_profile_without_using_its_cookie() {
    let w = World::new(login(2)).await;
    let id = w.login(false).await;
    let backend = Arc::new(ScriptHttp::new([json!({"code":301}).into()]));
    let reopened =
        MusicClient::open_with_http_backend(w.root.path(), None, backend.clone()).unwrap();
    let guest = reopened.restore_account().await.unwrap();
    reopened
        .remove_saved_account(&guest.epoch, &id)
        .await
        .unwrap();
    assert!(!reopened.account().unwrap().can_sign_out);
    assert!(w.registry.selection().unwrap().is_none());
    assert!(
        w.credentials(&id)
            .get("transport", "credentials", "session")
            .unwrap()
            .is_none()
    );
    assert_eq!(backend.calls().len(), 1);
    assert!(
        backend.calls()[0]
            .cookie
            .as_deref()
            .is_none_or(|s| !s.contains("MUSIC_U"))
    );
}

#[cfg(unix)]
#[tokio::test]
async fn redirected_directory_is_not_cleaned_and_missing_directory_can_be_removed() {
    let w = World::new(login(2).into_iter().chain(login(3))).await;
    let id = w.login(true).await;
    w.login(true).await;
    let path = w.root.path().join("sessions").join(&id);
    let moved = w.root.path().join("moved-fixture");
    std::fs::rename(&path, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &path).unwrap();
    let original = std::fs::read(moved.join("core.sqlite3")).unwrap();
    assert!(
        w.client
            .remove_saved_account(&w.epoch(), &id)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(moved.join("core.sqlite3")).unwrap(), original);
    assert!(w.registry.removal_pending(&id).unwrap());
    std::fs::remove_file(&path).unwrap();
    // The named session really no longer exists. Removal can finish without
    // following or erasing the unrelated relocated directory.
    w.client
        .remove_saved_account(&w.epoch(), &id)
        .await
        .unwrap();
    assert_eq!(std::fs::read(moved.join("core.sqlite3")).unwrap(), original);
    assert_eq!(w.registry.saved().unwrap().len(), 1);
}
