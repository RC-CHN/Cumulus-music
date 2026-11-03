mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, SessionStatus, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan, WireRoute,
        },
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Synthetic {
    code: AtomicUsize,
    calls: AtomicUsize,
}
impl HttpBackend for Synthetic {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert_eq!(plan.url.path(), "/eapi/w/nuser/account/get");
            self.calls.fetch_add(1, Ordering::SeqCst);
            let code = self.code.load(Ordering::SeqCst);
            let body = if code == 301 {
                json!({"code":301})
            } else {
                json!({"code":200,"profile":{"userId":1,"nickname":"Synthetic offline"}})
            };
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&body).unwrap(),
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}
struct Unbound;
impl Api for Unbound {
    fn request<'a>(
        &'a self,
        _: &'a str,
        _: serde_json::Value,
        _: bool,
    ) -> ApiFuture<'a, serde_json::Value> {
        Box::pin(async { panic!("unbound API must not be called") })
    }
}

async fn adopt(registry: &Arc<ProfileRegistry>, session: &AccountSession, backend: Arc<Synthetic>) {
    let candidate = registry
        .candidate_with_backend(ClientProfile::default(), Default::default(), None, backend)
        .unwrap();
    let url = ApiOrigin::Pc
        .request_url(WireRoute::Eapi, "/api/w/nuser/account/get")
        .unwrap();
    let credentials = candidate.credentials();
    let revision = credentials.request_credentials(&url).unwrap().revision;
    credentials
        .record_response(
            &url,
            &["MUSIC_U=synthetic-offline; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    candidate
        .verify_and_adopt(session, &session.token().unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn cold_grant_is_explicit_does_not_authenticate_and_expires_on_online_301() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let service = Arc::new(Synthetic::default());
    let online = AccountSession::new(Arc::new(Unbound), Some(registry.store()));
    adopt(&registry, &online, service.clone()).await;

    let cold = MusicClient::open_with_http_backend(dir.path(), None, service.clone()).unwrap();
    assert_eq!(cold.account().unwrap().status, SessionStatus::Unknown);
    let grant = cold.grant_cold_offline().unwrap();
    grant.validate().unwrap();
    assert_eq!(cold.account().unwrap().status, SessionStatus::Unknown);
    assert!(cold.playlists().await.is_err());
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
    let revoked = cold.grant_cold_offline().unwrap();
    revoked.revoke();
    assert!(matches!(revoked.validate(), Err(Error::StaleResponse)));
    service.code.store(301, Ordering::SeqCst);
    assert_eq!(
        cold.restore_account().await.unwrap().status,
        SessionStatus::Expired
    );
    assert!(grant.validate().is_err());
    assert!(cold.grant_cold_offline().is_err());
    let reopened = MusicClient::open_with_http_backend(dir.path(), None, service.clone()).unwrap();
    assert!(reopened.grant_cold_offline().is_err());
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn wrong_transport_slot_and_replaced_selection_never_grant_local_authority() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let service = Arc::new(Synthetic::default());
    let online = AccountSession::new(Arc::new(Unbound), Some(registry.store()));
    adopt(&registry, &online, service.clone()).await;
    let first = registry.selection().unwrap().unwrap();
    let old = MusicClient::open_with_http_backend(dir.path(), None, service.clone()).unwrap();
    let grant = old.grant_cold_offline().unwrap();

    let wrong = registry
        .candidate_with_backend(
            ClientProfile::default(),
            Default::default(),
            None,
            service.clone(),
        )
        .unwrap();
    let wrong_session = Arc::new(AccountSession::new(wrong.api(), Some(registry.store())));
    assert!(registry.grant_cold_offline(wrong_session).is_err());

    adopt(&registry, &online, service.clone()).await;
    assert_ne!(registry.selection().unwrap().unwrap(), first);
    assert!(grant.validate().is_err());
    assert!(old.grant_cold_offline().is_err());
    let replacement =
        MusicClient::open_with_http_backend(dir.path(), None, service.clone()).unwrap();
    replacement
        .grant_cold_offline()
        .unwrap()
        .validate()
        .unwrap();
    assert_eq!(service.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn native_plain_selection_opens_owned_published_bytes_and_rechecks_integrity() {
    use cumulus_core::{
        client::dto::{DownloadCommand, DownloadQuery, DownloadTab},
        download::service::DownloadOptions,
        membership::Quality,
    };
    use std::{fs, io::Read, time::Duration};
    const FLAC: &[u8] = include_bytes!("fixtures/tag_audio/seed.flac");

    let dir = tempfile::tempdir().unwrap();
    let media = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let service = Arc::new(Synthetic::default());
    let online = Arc::new(AccountSession::new(
        Arc::new(Unbound),
        Some(registry.store()),
    ));
    adopt(&registry, &online, service.clone()).await;
    // Establish the ordinary published task with a synthetic in-memory
    // catalog and transfer; the prior persisted native identity stays bound.
    let catalog = Arc::new(support::download::Server::new(FLAC, "flac"));
    online
        .adopt(catalog, json!({"userId":1}), &online.token().unwrap())
        .unwrap();
    let writer = MusicClient::with_session(online, registry.clone())
        .with_download_backend(Arc::new(support::download::Backend::new(FLAC.into())));
    writer
        .download_command(
            "2",
            DownloadCommand::Configure {
                root: media.path().join("media").to_string_lossy().into(),
                path_type: 2,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    let id = writer
        .download_command(
            "2",
            DownloadCommand::Start {
                song_ids: vec!["123".into()],
                options: DownloadOptions {
                    tags: false,
                    cover: false,
                    lyrics: false,
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap()
        .accepted
        .remove(0);
    let row = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let view = writer
                .downloads(
                    "2",
                    DownloadQuery {
                        tab: DownloadTab::Completed,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            if let Some(row) = view
                .tasks
                .into_iter()
                .find(|row| row.id == id && !row.cleanup_pending)
            {
                break row;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();

    let cold = MusicClient::open_with_http_backend(dir.path(), None, service.clone()).unwrap();
    let grant = cold.grant_cold_offline().unwrap();
    let page = grant.list_plain_audio(None, 1).unwrap();
    assert!(page.next.is_none());
    assert_eq!(
        serde_json::to_value(&page.rows).unwrap(),
        json!([{
            "id":id,"title":"Title","artist":"Artist","actualQuality":"lossless"
        }])
    );
    let audio = grant.open_plain_audio(&id).unwrap();
    assert_eq!(audio.quality(), Quality::Lossless);
    let mut bytes = Vec::new();
    audio.file().unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, FLAC);
    assert!(grant.open_plain_audio("../media").is_err());
    let mut task = registry
        .store()
        .get("1", "download_tasks_v1", &id)
        .unwrap()
        .unwrap();
    task["file_operation"] = json!("synthetic-file-operation");
    registry
        .store()
        .put("1", "download_tasks_v1", &id, &task)
        .unwrap();
    assert!(matches!(
        grant.open_plain_audio(&id),
        Err(Error::MutationBusy)
    ));
    assert!(grant.list_plain_audio(None, 1).unwrap().rows.is_empty());
    task.as_object_mut().unwrap().remove("file_operation");
    registry
        .store()
        .put("1", "download_tasks_v1", &id, &task)
        .unwrap();
    fs::write(row.path.as_ref().unwrap(), b"tampered").unwrap();
    assert_eq!(grant.list_plain_audio(None, 1).unwrap().rows.len(), 1);
    assert!(grant.open_plain_audio(&id).is_err());

    let store = registry.store();
    let path = std::path::Path::new(row.path.as_ref().unwrap())
        .canonicalize()
        .unwrap();
    let ledger_key =
        |path: &std::path::Path| hex::encode(Sha256::digest(path.to_str().unwrap().as_bytes()));
    let ledger = store
        .get("1", "offline_music_v1", &ledger_key(&path))
        .unwrap()
        .unwrap();
    let insert = |owner: &str, key: &str, encrypted: bool, published: bool| {
        let target = path.with_file_name(format!("{key}.flac"));
        fs::write(&target, FLAC).unwrap();
        let target = target.canonicalize().unwrap();
        let record_id = ledger_key(&target);
        let mut record = ledger.clone();
        record["id"] = json!(record_id);
        record["path"] = json!(target);
        record["task_id"] = json!(key);
        record["encrypted"] = json!(encrypted);
        let mut record_task = task.clone();
        record_task["id"] = json!(key);
        record_task["target"] = json!(target);
        record_task["encrypted"] = json!(encrypted);
        store
            .put(owner, "download_tasks_v1", key, &record_task)
            .unwrap();
        if published {
            store
                .put(owner, "offline_music_v1", &record_id, &record)
                .unwrap();
        }
    };
    insert("1", "extra-plain", false, true);
    insert("1", "encrypted", true, true);
    insert("1", "unpublished", false, false);
    insert("2", "foreign-owned", false, true);
    let first = grant.list_plain_audio(None, 1).unwrap();
    let cursor = first.next.as_ref().unwrap();
    let other_grant = cold.grant_cold_offline().unwrap();
    assert!(matches!(
        other_grant.list_plain_audio(Some(cursor), 1),
        Err(Error::StaleResponse)
    ));
    let second = grant.list_plain_audio(Some(cursor), 1).unwrap();
    let mut listed = [first.rows[0].id.as_str(), second.rows[0].id.as_str()];
    listed.sort_unstable();
    let mut expected = [id.as_str(), "extra-plain"];
    expected.sort_unstable();
    assert_eq!(listed, expected);
    if let Some(next) = second.next.as_ref() {
        assert!(
            grant
                .list_plain_audio(Some(next), 1)
                .unwrap()
                .rows
                .is_empty()
        );
    }
    assert!(grant.list_plain_audio(None, 65).is_err());
    assert!(grant.list_plain_audio(None, 0).is_err());
    grant.revoke();
    assert!(matches!(
        grant.list_plain_audio(None, 1),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(audio.file(), Err(Error::StaleResponse)));
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
}
