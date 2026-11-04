use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use tokio::sync::{Notify, oneshot};

const QR_CREATE: &str = "/api/login/qrcode/unikey";
const QR_POLL: &str = "/api/login/qrcode/client/login";
const QR: &str = "synthetic-import-code";
const INTERFACE_COOKIE: &str = "MUSIC_U=interface-proof; Path=/; Secure; HttpOnly";

fn reply(value: Value, cookies: &[&str]) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::to_vec(&value).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: cookies.iter().map(|value| (*value).into()).collect(),
        refresh_token: None,
        metadata: BTreeMap::new(),
    }
}

enum Response {
    Ready(HttpReply),
    Pending(oneshot::Receiver<HttpReply>),
}
impl From<HttpReply> for Response {
    fn from(value: HttpReply) -> Self {
        Self::Ready(value)
    }
}

#[derive(Default)]
struct Wire {
    calls: Mutex<Vec<(String, Value)>>,
    replies: Mutex<VecDeque<Response>>,
    dispatch: Notify,
}
impl HttpBackend for Wire {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert_eq!(plan.url.origin(), ORIGIN.url().origin());
            assert_eq!(plan.route, WireRoute::Eapi);
            let cookie = &plan.headers["Cookie"];
            assert!(!cookie.contains("pc-proof"));
            let form = url::form_urlencoded::parse(&plan.body).collect::<BTreeMap<_, _>>();
            let (path, data) = eapi::unwrap(&form["params"]).unwrap();
            assert_eq!(plan.url.path(), format!("/eapi/{}", &path[5..]));
            assert_eq!(data["e_r"], false);
            if path == IDENTITY_PATH {
                assert!(cookie.contains("MUSIC_U=interface-proof"));
                assert_eq!(data["header"]["MUSIC_U"], "interface-proof");
            } else {
                assert_eq!(data["type"], 3);
                assert!(!data["header"].as_object().unwrap().contains_key("MUSIC_U"));
            }
            self.calls.lock().unwrap().push((path, data));
            self.dispatch.notify_one();
            let response = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request or replay");
            Ok(match response {
                Response::Ready(reply) => reply,
                Response::Pending(release) => release.await.unwrap(),
            })
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    client: Arc<MusicClient>,
    wire: Arc<Wire>,
    session: Arc<AccountSession>,
    pc: Arc<MusicHttpClient>,
    owner: SessionToken,
    epoch: String,
}
impl Fixture {
    fn new(replies: impl IntoIterator<Item = Response>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let wire = Arc::new(Wire {
            replies: Mutex::new(replies.into_iter().collect()),
            ..Default::default()
        });
        let pc_credentials = Arc::new(Credentials::open(registry.store(), ApiOrigin::Pc).unwrap());
        let pc_url = ApiOrigin::Pc
            .request_url(WireRoute::Eapi, IDENTITY_PATH)
            .unwrap();
        pc_credentials
            .record_response(
                &pc_url,
                &["MUSIC_U=pc-proof; Path=/; Secure; HttpOnly".into()],
                None,
                &pc_credentials
                    .request_credentials(&pc_url)
                    .unwrap()
                    .revision,
            )
            .unwrap();
        let pc = Arc::new(
            MusicHttpClient::with_backend(
                pc_credentials,
                ClientProfile::default(),
                RequestOptions::default(),
                None,
                wire.clone(),
            )
            .unwrap(),
        );
        let session = Arc::new(AccountSession::new(pc.clone(), Some(registry.store())));
        session
            .adopt(pc.clone(), json!({"userId":7}), &session.token().unwrap())
            .unwrap();
        let owner = session.token().unwrap();
        let epoch = owner.generation.to_string();
        let client = Arc::new(
            MusicClient::with_session(session.clone(), registry)
                .with_login_providers(Some(wire.clone()), None),
        );
        Self {
            root,
            client,
            wire,
            session,
            pc,
            owner,
            epoch,
        }
    }

    async fn begin(&self) -> String {
        let view = self
            .client
            .external_import_auth_command(&self.epoch, ExternalImportAuthAction::Begin)
            .await
            .unwrap();
        assert_eq!(view.state, ExternalImportAuthState::WaitingScan);
        assert_eq!(view.poll_after_ms, Some(2_000));
        let matrix = serde_json::to_value(&view).unwrap();
        assert!(!matrix.to_string().contains(QR));
        assert!(!view.modules.is_empty());
        view.operation_id.unwrap()
    }

    async fn poll(&self, id: &str) -> ExternalImportAuthView {
        tokio::time::advance(POLL_INTERVAL).await;
        self.client
            .external_import_auth_command(
                &self.epoch,
                ExternalImportAuthAction::Poll {
                    operation_id: id.into(),
                },
            )
            .await
            .unwrap()
    }

    fn paths(&self) -> Vec<String> {
        self.wire
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.0.clone())
            .collect()
    }
}

#[tokio::test(start_paused = true)]
async fn independent_interface_qr_uses_its_own_cookies_and_keeps_pc_profile() {
    use ExternalImportAuthState as Phase;
    let f = Fixture::new([
        reply(json!({"code":200,"unikey":QR}), &[]).into(),
        reply(json!({"code":801}), &[]).into(),
        reply(json!({"code":802}), &[]).into(),
        reply(
            json!({"code":803}),
            &[
                INTERFACE_COOKIE,
                "MUSIC_U=forbidden; Domain=interfacepc.music.163.com; Path=/; Secure",
            ],
        )
        .into(),
        reply(json!({"code":200,"profile":{"userId":7}}), &[]).into(),
    ]);
    let id = f.begin().await;
    assert!(
        f.client
            .external_import_credentials(&f.owner)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.client
            .external_import_auth_command(
                &f.epoch,
                ExternalImportAuthAction::Poll {
                    operation_id: id.clone()
                }
            )
            .await
            .unwrap()
            .state,
        Phase::WaitingScan
    );
    assert_eq!(f.paths(), [QR_CREATE]);
    assert_eq!(f.poll(&id).await.state, Phase::WaitingScan);
    let view = f.poll(&id).await;
    assert_eq!(view.state, Phase::WaitingConfirmation);
    assert!(view.modules.is_empty());
    assert_eq!(f.poll(&id).await.state, Phase::Authorized);
    assert_eq!(
        f.paths(),
        [QR_CREATE, QR_POLL, QR_POLL, QR_POLL, IDENTITY_PATH]
    );
    assert_eq!(f.wire.calls.lock().unwrap()[1].1["key"], QR);
    let credentials = f
        .client
        .external_import_credentials(&f.owner)
        .unwrap()
        .unwrap();
    let target = ORIGIN.request_url(WireRoute::Weapi, IMPORT_PATH).unwrap();
    assert_eq!(
        credentials
            .request_credentials(&target)
            .unwrap()
            .unique_music_u(),
        Some("interface-proof")
    );
    assert!(
        credentials
            .request_credentials(&ApiOrigin::Pc.url())
            .is_err()
    );
    assert_eq!(f.session.token().unwrap(), f.owner);
    assert!(f.client.registry.selection().unwrap().is_none());
    assert!(!f.root.path().join("sessions").exists());
    assert_eq!(
        f.pc.credentials()
            .login_credentials()
            .unwrap()
            .token
            .as_deref(),
        Some("pc-proof")
    );

    f.session
        .adopt(f.pc.clone(), json!({"userId":8}), &f.owner)
        .unwrap();
    let next = f.session.token().unwrap();
    assert!(matches!(
        f.client.external_import_credentials(&f.owner),
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .external_import_credentials(&next)
            .unwrap()
            .is_none()
    );
    assert!(
        credentials
            .request_credentials(&target)
            .unwrap()
            .unique_music_u()
            .is_none()
    );
    assert_eq!(
        f.client
            .external_import_auth_view(&next.generation.to_string())
            .unwrap()
            .state,
        Phase::Idle
    );
}

#[tokio::test(start_paused = true)]
async fn uncertain_poll_only_reconciles_by_identity_and_rejects_different_account() {
    use ExternalImportAuthState as Phase;
    let mut cut = reply(Value::Null, &[INTERFACE_COOKIE]);
    cut.body_status = BodyStatus::Interrupted;
    let f = Fixture::new([
        reply(json!({"code":200,"unikey":QR}), &[]).into(),
        cut.into(),
        reply(json!({"code":200,"profile":{"userId":8}}), &[]).into(),
    ]);
    let id = f.begin().await;
    assert_eq!(f.poll(&id).await.state, Phase::IdentityPending);
    assert!(
        f.client
            .external_import_auth_command(
                &f.epoch,
                ExternalImportAuthAction::Poll {
                    operation_id: id.clone()
                }
            )
            .await
            .is_err()
    );
    let view = f
        .client
        .external_import_auth_command(
            &f.epoch,
            ExternalImportAuthAction::Reconcile { operation_id: id },
        )
        .await
        .unwrap();
    assert_eq!(view.state, Phase::WrongAccount);
    assert_eq!(f.paths(), [QR_CREATE, QR_POLL, IDENTITY_PATH]);
    assert!(
        f.client
            .external_import_credentials(&f.owner)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.session.token().unwrap(), f.owner);
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_poll_fences_late_cookie_and_old_operation() {
    use ExternalImportAuthState as Phase;
    let (send, receive) = oneshot::channel();
    let f = Fixture::new([
        reply(json!({"code":200,"unikey":QR}), &[]).into(),
        Response::Pending(receive),
    ]);
    let id = f.begin().await;
    tokio::time::advance(POLL_INTERVAL).await;
    let work = {
        let client = f.client.clone();
        let epoch = f.epoch.clone();
        let operation_id = id.clone();
        tokio::spawn(async move {
            client
                .external_import_auth_command(
                    &epoch,
                    ExternalImportAuthAction::Poll { operation_id },
                )
                .await
        })
    };
    while f.paths().len() < 2 {
        f.wire.dispatch.notified().await;
    }
    assert_eq!(
        f.client
            .external_import_auth_command(
                &f.epoch,
                ExternalImportAuthAction::Cancel {
                    operation_id: id.clone()
                }
            )
            .await
            .unwrap()
            .state,
        Phase::Idle
    );
    send.send(reply(json!({"code":803}), &[INTERFACE_COOKIE]))
        .ok()
        .unwrap();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        f.client
            .external_import_auth_command(
                &f.epoch,
                ExternalImportAuthAction::Reconcile { operation_id: id }
            )
            .await
            .is_err()
    );
    assert_eq!(f.paths(), [QR_CREATE, QR_POLL]);
    assert!(
        f.client
            .external_import_credentials(&f.owner)
            .unwrap()
            .is_none()
    );
}

#[tokio::test(start_paused = true)]
async fn an_eapi_only_cookie_cannot_authorize_the_weapi_target() {
    let f = Fixture::new([
        reply(json!({"code":200,"unikey":QR}), &[]).into(),
        reply(
            json!({"code":803}),
            &["MUSIC_U=interface-proof; Path=/eapi; Secure; HttpOnly"],
        )
        .into(),
    ]);
    let id = f.begin().await;
    assert_eq!(
        f.poll(&id).await.state,
        ExternalImportAuthState::Unconfirmed
    );
    assert!(
        f.client
            .external_import_credentials(&f.owner)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.paths(), [QR_CREATE, QR_POLL]);
    assert_eq!(f.session.token().unwrap(), f.owner);
}
