use super::*;
use crate::{
    account::AccountSession,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    effects::service::ModernLimiterPreflight,
    playback::engine::{Cancellation, NativeModernEffectSource, StereoPcmReader},
    protocol::api::{
        Api, ApiFuture, EffectCataloguePermission, EffectCatalogueRead, NativeEffectRegistration,
    },
    storage::StateStore,
};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

const DETAIL: &str = "/api/usertool/home/sound/info";
const AUTH: &str = "/api/vipauth/app/interests/auth/query";
const SET: &str = "/api/vipauth/app/interests/auth/set";
const RECYCLE: &str = "/api/vipauth/app/interests/recyclequery";

struct Step {
    path: &'static str,
    result: Result<Value>,
    after: Option<Box<dyn FnOnce() + Send>>,
}

fn step(path: &'static str, value: Value) -> Step {
    Step {
        path,
        result: Ok(value),
        after: None,
    }
}

fn auth(kind: u32) -> Step {
    step(
        AUTH,
        json!({"code":200,"data":[{"interestsId":300,"interestsType":127,
        "authResp":{"authResult":{"canUse":true,"canUseType":kind}}}]}),
    )
}

fn ack() -> Step {
    step(SET, json!({"code":200,"data":true}))
}

#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<Step>>,
    calls: Mutex<Vec<(&'static str, Value)>>,
    revision: Mutex<u64>,
    before_set: Mutex<Option<Cancellation>>,
}

impl Script {
    fn run(&self, path: &'static str, body: Value) -> Result<Value> {
        self.calls.lock().unwrap().push((path, body));
        let next = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unscripted effect request");
        assert_eq!(next.path, path);
        if let Some(after) = next.after {
            after();
        }
        next.result
    }
    fn count(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(actual, _)| *actual == path)
            .count()
    }
}

impl Api for Script {
    fn request<'a>(&'a self, _path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        panic!("scoped registration must never use an unguarded endpoint")
    }

    fn effect_catalogue_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.revision.lock().unwrap().to_string()))
    }

    fn request_effect_catalogue<'a>(
        &'a self,
        read: EffectCatalogueRead,
        revision: Option<&'a str>,
        permission: &'a EffectCataloguePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(
                self.effect_catalogue_credential_revision()?.as_deref(),
                revision
            );
            permission()?;
            let (path, body) = read.into_request();
            self.run(path, body)
        })
    }

    fn request_native_effect_registration<'a>(
        &'a self,
        write: NativeEffectRegistration,
        revision: &'a str,
        permission: &'a EffectCataloguePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(source) = self.before_set.lock().unwrap().take() {
                source.cancel();
            }
            permission().map_err(|err| Error::RequestNotSent(Box::new(err)))?;
            assert_eq!(
                self.effect_catalogue_credential_revision()?.as_deref(),
                Some(revision)
            );
            let (path, body) = write.into_request();
            assert_eq!(
                body,
                json!({"interestsType":127,"interestsId":"300","type":0})
            );
            self.run(path, body)
        })
    }
}

struct Origin(Vec<u8>);
impl MediaBackend for Origin {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(request.offset, 0);
            Ok(MediaReply {
                status: 200,
                headers: [("content-length".into(), self.0.len().to_string())].into(),
                body: Box::new(Bytes(std::io::Cursor::new(self.0.clone()))),
            })
        })
    }
}
struct Bytes(std::io::Cursor<Vec<u8>>);
impl MediaBody for Bytes {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(std::io::Read::read(&mut self.0, output)?) })
    }
}

struct World {
    _dir: tempfile::TempDir,
    api: Arc<Script>,
    auth: EffectAuthorizations,
    library: EffectsLibrary,
    token: SessionToken,
}
impl World {
    async fn create() -> (Self, Box<VerifiedModernLimiterMaterial>) {
        let material: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/effects-modern-limiter-preflight.json"
        )))
        .unwrap();
        let raw = hex::decode(material["modern"].as_str().unwrap()).unwrap();
        let detail = json!({"code":200,"data":{"soundList":[{"soundId":"300","soundType":10,"pcSoundDetail":{
            "id":300,"name":"Fixture","soundType":3,
            "vipAuthRespItem":{"authResult":{"canUse":true,"canUseType":11}},
            "soundMaterialList":[{"type":1,"size":raw.len(),"md5":hex::encode(Md5::digest(&raw)),"link":"https://m.test.music.126.net/first"}]
        }}]}});
        let api = Arc::new(Script::default());
        api.replies
            .lock()
            .unwrap()
            .extend([step(DETAIL, detail), auth(11)]);
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(AccountSession::new(
            api.clone(),
            Some(Arc::new(StateStore::open(dir.path()).unwrap())),
        ));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let token = session.token().unwrap();
        let library = EffectsLibrary::with_backend(session.clone(), Arc::new(Origin(raw))).unwrap();
        let ModernLimiterPreflight::Ready(material) = library
            .preflight_modern_limiter_for(&token, id(), Default::default())
            .await
            .unwrap()
        else {
            panic!("the original synthetic profile must qualify")
        };
        (
            Self {
                _dir: dir,
                api,
                auth: EffectAuthorizations::new(session).unwrap(),
                library,
                token,
            },
            material,
        )
    }
    fn replies(&self, replies: impl IntoIterator<Item = Step>) {
        self.api.replies.lock().unwrap().extend(replies);
    }
    async fn register(
        &self,
        material: Box<VerifiedModernLimiterMaterial>,
        source: ModernEffectSource,
    ) -> Result<NativeModernLimiterBinding> {
        self.auth
            .register_owned_modern_limiter_for(&self.library, &self.token, material, source)
            .await
    }
    fn lease(&self) -> Option<crate::effects::authorization::EffectLease> {
        self.auth.record_for(&self.token, id()).unwrap()
    }
}

fn id() -> EffectId {
    EffectId::new(300).unwrap()
}

fn source(world: &World, rate: u32) -> (ModernEffectSource, StereoPcmReader, Cancellation) {
    let lifetime = Cancellation::default();
    let (source, reader) = NativeModernEffectSource::test_source(rate, lifetime.clone()).unwrap();
    (
        ModernEffectSource::for_test(source, &world.auth.session, &world.token),
        reader,
        lifetime,
    )
}

#[tokio::test]
async fn modern_owned_registration_binds_exact_ack_material_and_source() {
    let (world, material) = World::create().await;
    world.replies([
        auth(11),
        ack(),
        auth(12),
        step(
            RECYCLE,
            json!({"code":200,"data":{"127":[{"interestsId":"300","canRecycle":false}]}}),
        ),
    ]);
    let (source, _reader, lifetime) = source(&world, 48_000);
    let binding = world.register(material, source).await.unwrap();
    assert_eq!(binding.effect(), id());
    assert!(current_non_trial(&world.lease().unwrap()));
    assert_eq!(world.api.count(SET), 1);
    assert_eq!(world.api.count(AUTH), 3);
    assert_eq!(world.api.count(RECYCLE), 1);
    world
        .auth
        .validate_owned_modern_limiter_for(&world.token, &binding)
        .await
        .unwrap();
    *world.api.revision.lock().unwrap() = 1;
    assert!(matches!(
        world
            .auth
            .validate_owned_modern_limiter_for(&world.token, &binding)
            .await,
        Err(Error::CredentialChanged)
    ));
    *world.api.revision.lock().unwrap() = 0;
    lifetime.cancel();
    assert!(
        world
            .auth
            .validate_owned_modern_limiter_for(&world.token, &binding)
            .await
            .is_err()
    );
    assert_eq!(world.api.count(SET), 1);
}

#[tokio::test]
async fn modern_owned_registration_rejects_stale_unsupported_or_trial_before_write() {
    for (rate, stale, trial) in [
        (32_000, false, false),
        (48_000, true, false),
        (48_000, false, true),
    ] {
        let (world, material) = World::create().await;
        let (source, _reader, lifetime) = source(&world, rate);
        if stale {
            lifetime.cancel();
        }
        if trial {
            world.replies([auth(50)]);
        }
        assert!(world.register(material, source).await.is_err());
        assert_eq!(world.api.count(SET), 0);
        assert!(world.lease().is_none());
    }
    let (world, material) = World::create().await;
    let (other, _) = World::create().await;
    let (foreign, _reader, _lifetime) = source(&other, 48_000);
    assert!(matches!(
        world.register(material, foreign).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(world.api.count(SET), 0);
    assert!(world.lease().is_none());
}

#[tokio::test]
async fn modern_owned_registration_keeps_unsent_acknowledged_and_unknown_distinct() {
    for case in ["before", "after", "unknown"] {
        let (world, material) = World::create().await;
        let (source, _reader, lifetime) = source(&world, 48_000);
        world.replies([auth(11)]);
        let expected = match case {
            "before" => {
                *world.api.before_set.lock().unwrap() = Some(lifetime);
                LeasePhase::StartRejected
            }
            "after" => {
                let mut confirmed = ack();
                confirmed.after = Some(Box::new(move || lifetime.cancel()));
                world.replies([confirmed]);
                LeasePhase::Registered
            }
            _ => {
                world.replies([Step {
                    path: SET,
                    result: Err(Error::Transport("sealed reply loss".into())),
                    after: None,
                }]);
                LeasePhase::StartUncertain
            }
        };
        let result = world.register(material, source).await;
        assert!(result.is_err());
        if case == "unknown" {
            assert!(matches!(result, Err(Error::MutationUncertain)));
        }
        let lease = world.lease().unwrap();
        assert_eq!(lease.phase, expected);
        assert_eq!(lease.start_acknowledged, case == "after");
        assert_eq!(world.api.count(SET), usize::from(case != "before"));
        assert_eq!(world.api.count(RECYCLE), 0);
        assert_eq!(
            world
                .auth
                .native_release_receipt_for(&world.token, id())
                .unwrap()
                .is_some(),
            case != "before"
        );
    }
}

#[tokio::test]
async fn modern_owned_registration_uses_single_guarded_pc_eapi_dispatch() {
    use crate::{
        account::credentials::Credentials,
        protocol::{
            http::{
                ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, MusicHttpClient,
                RequestPlan,
            },
            wire::eapi,
        },
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct Wire {
        calls: AtomicUsize,
        permitted: AtomicBool,
        interrupted: AtomicBool,
    }

    impl HttpBackend for Wire {
        fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply> {
            Box::pin(async move {
                assert!(self.permitted.swap(false, Ordering::AcqRel));
                assert_eq!(request.url.host_str(), Some("interfacepc.music.163.com"));
                assert_eq!(request.url.path(), "/eapi/vipauth/app/interests/auth/set");
                assert!(request.headers["Cookie"].contains("MUSIC_U=synthetic-modern-binding"));
                let params = url::form_urlencoded::parse(&request.body)
                    .find(|(name, _)| name == "params")
                    .unwrap()
                    .1;
                let (path, payload) = eapi::unwrap(&params).unwrap();
                assert_eq!(path, SET);
                // The PC EAPI form converter stringifies top-level scalar
                // business fields; the typed Api fixture above checks input.
                assert_eq!(payload["interestsType"], "127");
                assert_eq!(payload["interestsId"], "300");
                assert_eq!(payload["type"], "0");
                self.calls.fetch_add(1, Ordering::Relaxed);
                if self.interrupted.load(Ordering::Acquire) {
                    return Err(Error::Transport("synthetic backend interruption".into()));
                }
                Ok(HttpReply {
                    status: 200,
                    body: br#"{"code":200,"data":true}"#.to_vec(),
                    body_status: BodyStatus::Complete,
                    set_cookies: vec![],
                    refresh_token: None,
                    metadata: Default::default(),
                })
            })
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let credentials = Arc::new(
        Credentials::open(
            Arc::new(StateStore::open(dir.path()).unwrap()),
            ApiOrigin::Pc,
        )
        .unwrap(),
    );
    let target = ApiOrigin::Pc.url();
    credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-modern-binding; Secure; Path=/".into()],
            None,
            &credentials.request_credentials(&target).unwrap().revision,
        )
        .unwrap();
    let wire = Arc::new(Wire::default());
    let api = MusicHttpClient::with_backend(
        credentials,
        ClientProfile::default(),
        Default::default(),
        None,
        wire.clone(),
    )
    .unwrap();
    let revision = api.effect_catalogue_credential_revision().unwrap().unwrap();
    let permit = || {
        wire.permitted.store(true, Ordering::Release);
        Ok(())
    };
    let write = || NativeEffectRegistration::ordinary(id()).unwrap();
    let invalid = api
        .request_native_effect_registration(write(), &revision, &|| Err(Error::StaleResponse))
        .await;
    assert!(matches!(invalid, Err(Error::RequestNotSent(_))));
    let stale = api
        .request_native_effect_registration(write(), "not-the-current-credential", &permit)
        .await;
    assert!(matches!(stale, Err(Error::RequestNotSent(_))));
    assert_eq!(wire.calls.load(Ordering::Relaxed), 0);
    api.request_native_effect_registration(write(), &revision, &permit)
        .await
        .unwrap();
    assert_eq!(wire.calls.load(Ordering::Relaxed), 1);
    wire.interrupted.store(true, Ordering::Release);
    let interrupted = api
        .request_native_effect_registration(write(), &revision, &permit)
        .await;
    assert!(matches!(interrupted, Err(Error::Transport(_))));
    assert_eq!(wire.calls.load(Ordering::Relaxed), 2);
}
