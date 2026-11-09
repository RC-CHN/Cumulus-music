use super::*;
use crate::{
    protocol::http::{ApiOrigin, BodyStatus, HttpReply, RequestPlan},
    storage::StateStore,
};
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct Closed(AtomicUsize);

impl HttpBackend for Closed {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert!(plan.url.path().ends_with("/song/chorus"));
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(HttpReply {
                status: 200,
                body: br#"{"code":200,"data":[]}"#.to_vec(),
                body_status: BodyStatus::Complete,
                set_cookies: Vec::new(),
                refresh_token: None,
                metadata: BTreeMap::new(),
            })
        })
    }
}

#[tokio::test]
async fn chorus_requires_exact_cookie_revision_and_last_dispatch_permission() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let credentials = Arc::new(Credentials::open(store, ApiOrigin::Pc).unwrap());
    let target = ApiOrigin::Pc.url();
    let set_cookie = |value| {
        let snapshot = credentials.request_credentials(&target).unwrap();
        credentials
            .record_response(
                &target,
                &[format!("MUSIC_U={value}; Path=/; Secure")],
                None,
                &snapshot.revision,
            )
            .unwrap();
    };
    let backend = Arc::new(Closed::default());
    let client = MusicHttpClient::with_backend(
        credentials.clone(),
        ClientProfile::default(),
        RequestOptions::default(),
        None,
        backend.clone(),
    )
    .unwrap();
    assert_eq!(client.playback_chorus_revision().unwrap(), None);
    set_cookie("first-synthetic");
    let revision = client.playback_chorus_revision().unwrap().unwrap();
    let batch = || PlaybackChorusRead::new(vec![json!(21), json!("22")]).unwrap();
    let denied = client
        .request_playback_chorus(batch(), &revision, &|| Err(Error::StaleResponse))
        .await;
    assert!(matches!(denied, Err(Error::StaleResponse)));
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);

    set_cookie("second-synthetic");
    let stale = client
        .request_playback_chorus(batch(), &revision, &|| Ok(()))
        .await;
    assert!(matches!(stale, Err(Error::CredentialChanged)));
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    let fresh = client.playback_chorus_revision().unwrap().unwrap();
    assert_ne!(revision, fresh);
    assert_eq!(
        client
            .request_playback_chorus(batch(), &fresh, &|| Ok(()))
            .await
            .unwrap()["data"],
        json!([])
    );
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
}
