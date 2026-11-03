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
    task.as_object_mut().unwrap().remove("file_operation");
    registry
        .store()
        .put("1", "download_tasks_v1", &id, &task)
        .unwrap();
    fs::write(row.path.unwrap(), b"tampered").unwrap();
    assert!(grant.open_plain_audio(&id).is_err());
    grant.revoke();
    assert!(matches!(audio.file(), Err(Error::StaleResponse)));
    assert_eq!(service.calls.load(Ordering::SeqCst), 1);
}
