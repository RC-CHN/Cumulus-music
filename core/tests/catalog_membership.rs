use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
struct Backend {
    collected: AtomicBool,
    drop_reply: AtomicBool,
    writes: AtomicUsize,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if path.ends_with("sublist") {
                return Ok(
                    json!({"code":200,"more":false,"data":if self.collected.load(Ordering::SeqCst){vec![json!({"id":12})]}else{vec![]}}),
                );
            }
            if matches!(
                path,
                "/api/artist/sub" | "/api/artist/unsub" | "/api/album/sub" | "/api/album/unsub"
            ) {
                self.writes.fetch_add(1, Ordering::SeqCst);
                self.collected
                    .store(path.ends_with("/sub"), Ordering::SeqCst);
                if self.drop_reply.swap(false, Ordering::SeqCst) {
                    return Err(Error::Transport("lost fixture reply".into()));
                }
                return Ok(json!({"code":200}));
            }
            panic!("unexpected {path}");
        })
    }
}
#[tokio::test]
async fn catalog_memberships_are_confirmed_by_reads_and_noop_intents_do_not_write() {
    for resource in [
        CatalogResource::Artist { id: "12".into() },
        CatalogResource::Album { id: "12".into() },
    ] {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let api = Arc::new(Backend::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let client = MusicClient::with_session(session, registry);
        assert!(
            client
                .catalog_membership("1", resource.clone(), CatalogMembershipAction::Inspect)
                .await
                .unwrap()
                .value
                .operation
                .is_none()
        );
        let view = client
            .catalog_membership(
                "1",
                resource.clone(),
                CatalogMembershipAction::Set { desired: true },
            )
            .await
            .unwrap()
            .value;
        assert_eq!(
            serde_json::to_value(view).unwrap()["operation"],
            json!({"phase":"confirmed","desired":true,"observed":true})
        );
        let noop = client
            .catalog_membership(
                "1",
                resource,
                CatalogMembershipAction::Set { desired: true },
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(noop).unwrap()["value"]["operation"]["phase"],
            "unchanged"
        );
        assert_eq!(api.writes.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn lost_catalog_write_is_durable_blocks_resend_and_recovers_by_read_only_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    let resource = CatalogResource::Artist { id: "12".into() };
    api.drop_reply.store(true, Ordering::SeqCst);
    assert!(
        client
            .catalog_membership(
                "1",
                resource.clone(),
                CatalogMembershipAction::Set { desired: true }
            )
            .await
            .is_err()
    );
    let inspect = client
        .catalog_membership("1", resource.clone(), CatalogMembershipAction::Inspect)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(inspect).unwrap()["value"]["operation"]["phase"],
        "uncertain"
    );
    assert!(matches!(
        client
            .catalog_membership(
                "1",
                resource.clone(),
                CatalogMembershipAction::Set { desired: true }
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    let reopened = MusicClient::with_session(
        session.clone(),
        ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap(),
    );
    let confirmed = reopened
        .catalog_membership("1", resource.clone(), CatalogMembershipAction::Reconcile)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(confirmed).unwrap()["value"]["operation"]["observed"],
        true
    );
    assert_eq!(api.writes.load(Ordering::SeqCst), 1);
    session
        .adopt(api, json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        client
            .catalog_membership(
                "1",
                resource.clone(),
                CatalogMembershipAction::Set { desired: false }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        client
            .catalog_membership("2", resource, CatalogMembershipAction::Inspect)
            .await
            .unwrap()
            .value
            .operation
            .is_none()
    );
}
