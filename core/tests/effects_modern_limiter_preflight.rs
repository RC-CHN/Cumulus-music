mod support;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::transfer::{RunOptions, TransferManager, TransferStatus},
    effects::{
        catalog::EffectId,
        dsp::Format,
        service::{
            EffectsLibrary, MaterialHandle, ModernLimiterPreflight, StandaloneParametricPreflight,
        },
    },
    protocol::api::{Api, ApiFuture, EffectCataloguePermission, EffectCatalogueRead},
    storage::StateStore,
};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::{
    Reply, ScriptApi, gate,
    transfer::{Origin, ReadGate},
};

const FIRST: &str = "https://m.test.music.126.net/first";
const SECOND: &str = "https://m.test.music.126.net/second";
const DETAIL: &str = "/api/usertool/home/sound/info";
const AUTH: &str = "/api/vipauth/app/interests/auth/query";

fn ncae(case: &str) -> Vec<u8> {
    let fixtures: Value = serde_json::from_str(include_str!(
        "fixtures/effects-modern-limiter-preflight.json"
    ))
    .unwrap();
    hex::decode(fixtures[case].as_str().unwrap()).unwrap()
}

fn id() -> EffectId {
    EffectId::new(300).unwrap()
}

fn detail(first: &[u8], second: &[u8]) -> Value {
    let material = |bytes: &[u8], link: &str| {
        json!({
            "type":1,"size":bytes.len(),"md5":hex::encode(Md5::digest(bytes)),"link":link
        })
    };
    json!({"code":200,"data":{"soundList":[{"soundId":"300","soundType":10,"pcSoundDetail":{
        "id":300,"name":"Fixture","soundType":3,
        "vipAuthRespItem":{"authResult":{"canUse":true,"canUseType":11}},
        "soundMaterialList":[material(first, FIRST),material(second, SECOND)]
    }}]}})
}

fn authorization(allow: Value, kind: Value) -> Value {
    json!({"code":200,"data":[{"interestsId":300,"interestsType":127,
        "authResp":{"authResult":{"canUse":allow,"canUseType":kind}}}]})
}

fn allowed() -> Value {
    authorization(json!(true), json!(11))
}

struct ScopedApi {
    script: ScriptApi,
    revision: Mutex<Option<String>>,
    guarded_dispatches: Mutex<Vec<(String, String)>>,
}

impl ScopedApi {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Arc<Self> {
        Arc::new(Self {
            script: ScriptApi::new(replies),
            revision: Mutex::new(Some("credential-1".into())),
            guarded_dispatches: Mutex::new(vec![]),
        })
    }

    fn rotate(&self, revision: Option<&str>) {
        *self.revision.lock().unwrap() = revision.map(str::to_owned);
    }
}

impl Api for ScopedApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        self.script.request(path, payload, cache)
    }

    fn effect_catalogue_credential_revision(&self) -> Result<Option<String>> {
        Ok(self.revision.lock().unwrap().clone())
    }

    fn request_effect_catalogue<'a>(
        &'a self,
        read: EffectCatalogueRead,
        revision: Option<&'a str>,
        current: &'a EffectCataloguePermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            current()?;
            let revision = revision.expect("material request must pin the credential revision");
            assert_eq!(self.revision.lock().unwrap().as_deref(), Some(revision));
            let (path, payload) = read.into_request();
            self.guarded_dispatches
                .lock()
                .unwrap()
                .push((path.into(), revision.into()));
            self.call(path, payload, false, &[200]).await
        })
    }
}

fn session(api: Arc<ScopedApi>, store: Arc<StateStore>) -> Arc<AccountSession> {
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    session
}

struct World {
    _temp: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ScopedApi>,
    session: Arc<AccountSession>,
    token: SessionToken,
    origin: Arc<Origin>,
    library: EffectsLibrary,
}

impl World {
    fn new(data: Vec<u8>, replies: impl IntoIterator<Item = Reply>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let api = ScopedApi::new(replies);
        let session = session(api.clone(), store.clone());
        let token = session.token().unwrap();
        let origin = Arc::new(Origin::new(data));
        let library = EffectsLibrary::with_backend(session.clone(), origin.clone()).unwrap();
        Self {
            _temp: temp,
            store,
            api,
            session,
            token,
            origin,
            library,
        }
    }

    async fn preflight(&self) -> Result<ModernLimiterPreflight> {
        self.library
            .preflight_modern_limiter_for(&self.token, id(), RunOptions::default())
            .await
    }

    fn transfers(&self) -> TransferManager {
        TransferManager::with_backend(
            self.store.clone(),
            "1".into(),
            self.origin.clone(),
            64 * 1024 * 1024,
        )
        .unwrap()
    }

    fn assert_reads(&self, pairs: usize, guarded: usize) {
        let calls = self.api.script.calls();
        assert_eq!(calls.len(), pairs * 2);
        for pair in calls.chunks_exact(2) {
            assert_eq!(pair[0].path, DETAIL);
            let req = pair[0].payload["reqListStr"].as_str().unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(req).unwrap(),
                json!([{"soundId":"300","soundType":10}])
            );
            assert_eq!(pair[1].path, AUTH);
            assert_eq!(
                pair[1].payload,
                json!({"queryInfo":{"interestsMap":{"127":[300]}}})
            );
            assert!(pair.iter().all(|call| !call.cache));
        }
        let dispatches = self.api.guarded_dispatches.lock().unwrap();
        assert_eq!(dispatches.len(), guarded * 2);
        for pair in dispatches.chunks_exact(2) {
            assert_eq!(pair[0], (DETAIL.into(), "credential-1".into()));
            assert_eq!(pair[1], (AUTH.into(), "credential-1".into()));
        }
    }
}

#[tokio::test]
async fn modern_material_seals_one_verified_attempt_without_registering_or_falling_back() {
    let bytes = ncae("modern");
    let detail = detail(&bytes, &ncae("extraOff"));
    let world = World::new(
        bytes,
        (0..5).flat_map(|_| [detail.clone().into(), allowed().into()]),
    );
    assert!(matches!(
        world
            .library
            .preflight_modern_limiter_for(
                &world.token,
                id(),
                RunOptions {
                    byte_budget: Some(17),
                    ..Default::default()
                }
            )
            .await
            .unwrap(),
        ModernLimiterPreflight::Pending { bytes: 17 }
    ));
    let ModernLimiterPreflight::Ready(first) = world.preflight().await.unwrap() else {
        panic!("expected verified modern graph")
    };
    assert_eq!(first.effect(), id());
    for (rate, channels, capacity, supported) in [
        (44_100.0, 2, 2048, true),
        (48_000.0, 2, 2048, true),
        (96_000.0, 2, 2048, false),
        (48_000.0, 1, 2048, false),
        (48_000.0, 2, 1024, false),
    ] {
        assert_eq!(
            first.supports_graph_format(Format::new(rate, channels, capacity).unwrap()),
            supported
        );
    }
    world
        .library
        .validate_modern_limiter_preflight_for(&world.token, &first)
        .await
        .unwrap();
    let transfers = world.transfers();
    let first_job = transfers.list().unwrap().pop().unwrap();
    let key = first_job
        .resource_id
        .as_ref()
        .unwrap()
        .strip_prefix("effect-material:")
        .unwrap();
    let record = world
        .store
        .get("1", "effects-materials-v1", key)
        .unwrap()
        .unwrap();
    let handle: MaterialHandle = serde_json::from_value(record["handle"].clone()).unwrap();
    let calls_after_resume = world.origin.calls().len();
    assert_eq!(calls_after_resume, 2);
    assert!(matches!(
        world.preflight().await.unwrap(),
        ModernLimiterPreflight::Ready(_)
    ));
    assert_eq!(world.origin.calls().len(), calls_after_resume);
    assert!(matches!(
        world
            .library
            .preflight_parametric_activation_for(&world.token, id(), RunOptions::default())
            .await
            .unwrap(),
        StandaloneParametricPreflight::Unsupported
    ));
    assert_eq!(world.origin.calls().len(), calls_after_resume);

    let other_session = session(world.api.clone(), world.store.clone());
    assert_eq!(other_session.token().unwrap(), world.token);
    let other = EffectsLibrary::with_backend(other_session, world.origin.clone()).unwrap();
    assert!(matches!(
        other
            .validate_modern_limiter_preflight_for(&world.token, &first)
            .await,
        Err(Error::StaleResponse)
    ));
    world.library.remove_for(&world.token, &handle).unwrap();
    assert!(matches!(
        world
            .library
            .validate_modern_limiter_preflight_for(&world.token, &first)
            .await,
        Err(Error::State(_))
    ));
    let ModernLimiterPreflight::Ready(second) = world.preflight().await.unwrap() else {
        panic!("same bytes can be reacquired explicitly")
    };
    assert!(
        matches!(
            world
                .library
                .validate_modern_limiter_preflight_for(&world.token, &first)
                .await,
            Err(Error::State(_))
        ),
        "same digest cannot revive a removed attempt"
    );
    world
        .library
        .validate_modern_limiter_preflight_for(&world.token, &second)
        .await
        .unwrap();
    let new_job = transfers
        .list()
        .unwrap()
        .into_iter()
        .find(|job| job.status == TransferStatus::Complete)
        .unwrap();
    assert_ne!(new_job.id, first_job.id);
    let mut changed = std::fs::read(&new_job.target).unwrap();
    changed[8] ^= 1;
    std::fs::write(&new_job.target, changed).unwrap();
    assert!(matches!(
        world
            .library
            .validate_modern_limiter_preflight_for(&world.token, &second)
            .await,
        Err(Error::Media(_))
    ));
    assert!(
        world
            .origin
            .calls()
            .iter()
            .all(|request| request.url.as_str() == FIRST)
    );
    assert_eq!(world.origin.calls().len(), 3);
    world.assert_reads(5, 4);
}

#[tokio::test]
async fn supported_second_material_cannot_rescue_unsupported_first_material() {
    let modern = ncae("modern");
    let mut wrong_type = modern.clone();
    wrong_type[14] = 2;
    let old: Value =
        serde_json::from_str(include_str!("fixtures/effects-service-materials.json")).unwrap();
    let old = old["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "first")
        .unwrap();
    for first in [
        ncae("extraOff"),
        ncae("legacyUsableModernBroken"),
        wrong_type,
        hex::decode(old["encoded_hex"].as_str().unwrap()).unwrap(),
    ] {
        let detail = detail(&first, &modern);
        let world = World::new(first, [detail.into(), allowed().into()]);
        assert!(matches!(
            world.preflight().await.unwrap(),
            ModernLimiterPreflight::Unsupported
        ));
        assert_eq!(world.origin.calls().len(), 1);
        assert_eq!(world.origin.calls()[0].url.as_str(), FIRST);
        world.assert_reads(1, 1);
    }
}

#[tokio::test]
async fn denied_trial_missing_scope_and_corrupt_material_never_issue_receipts() {
    let bytes = ncae("modern");
    let detail = detail(&bytes, &bytes);
    for (can_use, kind, expected) in [
        (json!(false), json!(11), "denied"),
        (json!(true), json!(50), "trial"),
        (json!(true), Value::Null, "trial"),
        (json!("true"), json!(11), "malformed"),
    ] {
        let world = World::new(
            bytes.clone(),
            [detail.clone().into(), authorization(can_use, kind).into()],
        );
        match (world.preflight().await, expected) {
            (Err(Error::State("effect material is not authorized")), "denied")
            | (Err(Error::State("effect material requires confirmed non-trial rights")), "trial")
            | (Err(Error::Response("missing effect authorization decision")), "malformed") => (),
            _ => panic!("unexpected preflight result for {expected}"),
        }
        assert!(world.origin.calls().is_empty());
        assert!(world.transfers().list().unwrap().is_empty());
        world.assert_reads(1, 1);
    }
    let world = World::new(bytes.clone(), []);
    assert!(matches!(
        world
            .library
            .preflight_modern_limiter_for(
                &world.token,
                EffectId::new(201).unwrap(),
                RunOptions::default()
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    world.api.rotate(None);
    assert!(matches!(
        world.preflight().await,
        Err(Error::State(
            "scoped effect material credentials are unavailable"
        ))
    ));
    assert!(world.origin.calls().is_empty());
    world.assert_reads(0, 0);
    let mut invalid_ncae = bytes;
    invalid_ncae[0] = b'X';
    let corrupt_detail = self::detail(&invalid_ncae, &invalid_ncae);
    let world = World::new(invalid_ncae, [corrupt_detail.into(), allowed().into()]);
    assert!(matches!(
        world.preflight().await,
        Err(Error::Media("invalid or truncated NCAE header"))
    ));
    world.assert_reads(1, 1);
}

#[tokio::test]
async fn credential_change_fences_each_live_stage_and_cached_receipt_without_an_account_switch() {
    let bytes = ncae("modern");
    let detail = detail(&bytes, &bytes);
    for at_detail in [true, false] {
        let (pending, finish) = gate();
        let (replies, response, calls) = if at_detail {
            (vec![pending], detail.clone(), 1)
        } else {
            (vec![detail.clone().into(), pending], allowed(), 2)
        };
        let world = World::new(bytes.clone(), replies);
        let worker = {
            let library = world.library.clone();
            let token = world.token.clone();
            tokio::spawn(async move {
                library
                    .preflight_modern_limiter_for(&token, id(), RunOptions::default())
                    .await
            })
        };
        world.api.script.wait_for_calls(calls).await;
        world.api.rotate(Some("credential-2"));
        finish.send(Ok(response)).unwrap();
        assert!(matches!(
            worker.await.unwrap(),
            Err(Error::CredentialChanged)
        ));
        assert_eq!(world.session.token().unwrap(), world.token);
        assert_eq!(world.api.script.calls().len(), calls);
        assert!(world.origin.calls().is_empty());
        assert!(world.transfers().list().unwrap().is_empty());
    }
    let world = World::new(
        bytes.clone(),
        [
            detail.clone().into(),
            allowed().into(),
            detail.clone().into(),
            allowed().into(),
        ],
    );
    let read_gate = Arc::new(ReadGate::default());
    world.origin.settings.lock().unwrap().gate = Some(read_gate.clone());
    let worker = {
        let library = world.library.clone();
        let token = world.token.clone();
        tokio::spawn(async move {
            library
                .preflight_modern_limiter_for(&token, id(), RunOptions::default())
                .await
        })
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        read_gate.reached.notified(),
    )
    .await
    .unwrap();
    world.api.rotate(Some("credential-2"));
    read_gate.release.notify_one();
    assert!(matches!(
        worker.await.unwrap(),
        Err(Error::CredentialChanged)
    ));
    let job = world.transfers().list().unwrap().pop().unwrap();
    // CredentialChanged retains the transfer's established retryable Error
    // state. StaleResponse alone uses the session-generation Paused branch.
    assert_eq!(job.status, TransferStatus::Error);
    assert_eq!(job.last_error, Some(Error::CredentialChanged.to_string()));
    assert_eq!(job.bytes, 0);
    assert!(!job.target.exists());
    world.assert_reads(1, 1);
    world.origin.settings.lock().unwrap().gate = None;
    assert!(matches!(
        world.preflight().await.unwrap(),
        ModernLimiterPreflight::Ready(_)
    ));
    let resumed = world.transfers().list().unwrap().pop().unwrap();
    assert_eq!(resumed.id, job.id);
    assert_eq!(resumed.status, TransferStatus::Complete);
    assert_eq!(world.origin.calls().len(), 2);
    assert_eq!(
        &world.api.guarded_dispatches.lock().unwrap()[2..],
        &[
            (DETAIL.into(), "credential-2".into()),
            (AUTH.into(), "credential-2".into())
        ],
    );

    let world = World::new(bytes, [detail.into(), allowed().into()]);
    let ModernLimiterPreflight::Ready(receipt) = world.preflight().await.unwrap() else {
        panic!("expected initial receipt")
    };
    world.api.rotate(Some("credential-2"));
    assert!(matches!(
        world
            .library
            .validate_modern_limiter_preflight_for(&world.token, &receipt)
            .await,
        Err(Error::CredentialChanged)
    ));
    world
        .session
        .adopt(world.api.clone(), json!({"userId":2}), &world.token)
        .unwrap();
    assert!(matches!(
        world
            .library
            .validate_modern_limiter_preflight_for(&world.session.token().unwrap(), &receipt)
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(world.origin.calls().len(), 1);
    world.assert_reads(1, 1);
}
