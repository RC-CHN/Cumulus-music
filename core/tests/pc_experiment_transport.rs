mod support;
use cumulus_core::{
    Error,
    account::credentials::Credentials,
    client::{MusicClient, pc_experiments::OrdinarySkipAssignment},
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{ApiOrigin, ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        pc_experiments::{ASSIGNMENT_PATH, assignment_arguments},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::http::{HttpResponse, ScriptHttp, gate, reply};

struct ClosedConfig;
impl ConfigBackend for ClosedConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("fixture must not fetch remote configuration") })
    }
}
impl TokenProvider for ClosedConfig {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { panic!("assignment is not protected in the fixture") })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    client: Arc<MusicClient>,
    http: Arc<ScriptHttp>,
    epoch: String,
}
impl Fixture {
    async fn new(responses: impl IntoIterator<Item = HttpResponse>, native: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let account = root.path().join("profile");
        let store = Arc::new(StateStore::open(&account).unwrap());
        let credentials = Credentials::open(store, ApiOrigin::Pc).unwrap();
        let target = ApiOrigin::Pc.url();
        let original = credentials.request_credentials(&target).unwrap();
        credentials
            .record_response(
                &target,
                &["MUSIC_U=synthetic-pc-experiment; Path=/; Secure".into()],
                None,
                &original.revision,
            )
            .unwrap();
        let http = Arc::new(ScriptHttp::new(
            [json!({"code":200,"profile":{"userId":71,"nickname":"合成账号"}}).into()]
                .into_iter()
                .chain(responses),
        ));
        let context = native.then(|| {
            let service = PcConfig::new(
                Arc::new(StateStore::open(root.path().join("config")).unwrap()),
                Arc::new(ClosedConfig),
            )
            .unwrap();
            Arc::new(
                RequestContext::from_pc(service, "3.1.42.205594", Arc::new(ClosedConfig)).unwrap(),
            )
        });
        let profile = if native {
            ClientProfile {
                user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                os_version: "Microsoft Windows 10".into(),
                client_sign: Some("synthetic-signature".into()),
            }
        } else {
            ClientProfile::default()
        };
        let client = Arc::new(
            MusicClient::open_with_request_context(
                &root.path().join("host"),
                Some(&account),
                profile,
                Some(http.clone()),
                context,
            )
            .unwrap(),
        );
        let epoch = client.restore_account().await.unwrap().epoch;
        Self {
            _root: root,
            client,
            http,
            epoch,
        }
    }
}

fn enabled() -> Value {
    json!({"code":200,"data":[{"expName":"PH-PC-skipSongs2","expGroupName":"t1"}]})
}

#[tokio::test(start_paused = true)]
async fn signed_native_pc_read_keeps_its_scope_and_expires_at_exact_refresh_boundary() {
    let f = Fixture::new(
        [enabled().into(), json!({"code":200,"data":[]}).into()],
        true,
    )
    .await;
    let client = &f.client;
    assert_eq!(
        client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    tokio::time::advance(Duration::from_secs(1799)).await;
    assert_eq!(
        client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    assert_eq!(f.http.calls().len(), 2);
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(
        client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Control
    );
    let calls = f.http.calls();
    assert_eq!(calls.len(), 3);
    for call in &calls[1..] {
        assert_eq!(call.path, ASSIGNMENT_PATH);
        assert_eq!(call.payload["expNames"], assignment_arguments()["expNames"]);
        assert!(
            call.cookie
                .as_ref()
                .is_some_and(|cookie| cookie.contains("MUSIC_U=synthetic-pc-experiment"))
        );
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test(start_paused = true)]
async fn failures_after_expiry_do_not_grant_an_old_group_or_send_a_fallback() {
    let f = Fixture::new([enabled().into(), json!({"code":403}).into()], true).await;
    assert_eq!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    tokio::time::advance(Duration::from_secs(1800)).await;
    assert!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch)
            .await
            .is_err()
    );
    assert_eq!(f.http.calls().len(), 3);
    let unsigned = Fixture::new([], false).await;
    assert!(
        unsigned
            .client
            .official_pc_ordinary_skip_assignment(&unsigned.epoch)
            .await
            .is_err()
    );
    assert_eq!(unsigned.http.calls().len(), 1);
}

#[tokio::test]
async fn logout_cancels_held_response_without_carrying_group_to_new_epoch() {
    let (held, answer) = gate();
    let f = Fixture::new([held], true).await;
    let client = f.client.clone();
    let epoch = f.epoch.clone();
    let pending =
        tokio::spawn(async move { client.official_pc_ordinary_skip_assignment(&epoch).await });
    f.http.wait(2).await;
    f.client.logout_account(&f.epoch, false).await.unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert!(answer.send(Ok(reply(enabled()))).is_err());
    let new_epoch = f.client.account().unwrap().epoch;
    assert_ne!(new_epoch, f.epoch);
    assert!(
        f.client
            .official_pc_ordinary_skip_assignment(&new_epoch)
            .await
            .is_err()
    );
    assert_eq!(f.http.calls().len(), 2);
}
