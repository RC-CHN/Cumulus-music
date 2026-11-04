mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    download::transfer::{RunOptions, TransferManager, TransferStatus},
    effects::{
        api::EffectsApi,
        catalog::{CatalogueKind, EffectId, MaterialDescriptor, Sound},
        rights::{AuthQuery, Authorization, HostRights, decide_access},
        service::{EffectsLibrary, MaterialPhase},
    },
    storage::StateStore,
};
use md5::Md5;
use serde_json::{Value, json};
use sha2::Digest;
use std::sync::Arc;
use support::{
    Reply, ScriptApi, gate,
    transfer::{Origin, ReadGate},
};
fn sample(name: &str) -> (Vec<u8>, Value) {
    let all: Value =
        serde_json::from_str(include_str!("fixtures/effects-service-materials.json")).unwrap();
    let s = all["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap();
    (
        hex::decode(s["encoded_hex"].as_str().unwrap()).unwrap(),
        s["parameters"].clone(),
    )
}
fn id() -> EffectId {
    EffectId::new(300).unwrap()
}
fn sound(bytes: &[u8], url: &str) -> Value {
    json!({"id":300,"name":"Fixture","soundType":3,"vipAuthRespItem":{"interestsVipType":6,"authResult":{"canUse":true}},"soundMaterialList":[{"type":1,"size":bytes.len(),"md5":hex::encode(Md5::digest(bytes)),"link":url}]})
}
fn detail(sound: &Value) -> Reply {
    json!({"code":200,"data":{"soundList":[{"soundId":"300","soundType":10,"pcSoundDetail":sound}]}}).into()
}
fn auth(allow: bool) -> Reply {
    json!({"code":200,"data":[{"interestsId":300,"interestsType":127,"authResp":{"interestsVipType":6,"authResult":{"canUse":allow,"canUseType":11}}}]}).into()
}
fn pair(sound: &Value, allow: bool) -> [Reply; 2] {
    [detail(sound), auth(allow)]
}
fn world(api: Arc<ScriptApi>, store: Arc<StateStore>) -> Arc<AccountSession> {
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    session
}
fn transfer(store: Arc<StateStore>, backend: Arc<Origin>) -> TransferManager {
    TransferManager::with_backend(store, "1".into(), backend, 64 * 1024 * 1024).unwrap()
}
const URL: &str = "https://m.test.music.126.net/material?lease=first";

#[tokio::test]
async fn headphone_material_uses_only_the_exact_brand_model_and_verifies_the_graph() {
    use cumulus_core::effects::service::DeviceMaterialCheck;
    let brand = EffectId::new(7002).unwrap();
    for (sample_name, expected) in [
        ("first", DeviceMaterialCheck::StandalonePeq),
        ("replacement", DeviceMaterialCheck::UnsupportedGraph),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let (bytes, _) = sample(sample_name);
        let row = json!({"soundId":"300","soundType":2,"deviceModel":"Synthetic",
            "size":bytes.len(),"soundMd5":hex::encode(Md5::digest(&bytes)),"deviceSoundUrl":URL});
        let response = |records: Vec<Value>| json!({"code":200,"data":records}).into();
        let api = Arc::new(ScriptApi::new([
            response(vec![row.clone()]),
            response(vec![row.clone()]),
            response(vec![row.clone(), row.clone()]),
            response(vec![row.clone()]),
        ]));
        let session = world(api.clone(), store);
        let token = session.token().unwrap();
        let origin = Arc::new(Origin::new(bytes));
        let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
        let check =
            |model| library.check_device_material_for(&token, brand, model, RunOptions::default());
        assert!(matches!(
            check(EffectId::new(301).unwrap()).await,
            Err(Error::Response(_))
        ));
        assert!(origin.calls().is_empty());
        assert_eq!(check(id()).await.unwrap(), expected);
        assert!(matches!(
            check(id()).await,
            Err(Error::Response("duplicate effect device identity"))
        ));
        assert_eq!(check(id()).await.unwrap(), expected);
        assert_eq!(origin.calls().len(), 1);
        let calls = api.calls();
        assert_eq!(calls.len(), 4);
        assert!(calls.iter().all(|call| call.path
            == "/api/usertool/home/soundeffect/devicesound/list"
            && call.payload == json!({"deviceManagerId":7002})));
    }
}

#[tokio::test]
async fn optional_headphone_tuning_never_blocks_first_play_after_a_safe_detach() {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, dto::PlaySelection},
        membership::Quality,
        playback::controller::{ParametricApplication, ParametricBindingStatus, PlaybackStatus},
        protocol::http::ApiOrigin,
    };
    use std::{sync::atomic::Ordering, time::Duration};

    for cancel_before_load in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(temp.path(), ApiOrigin::Pc).unwrap();
        let (bytes, _) = sample("first");
        let model_row = json!({"code":200,"data":[{"soundId":300,"soundType":2,"deviceModel":"  合成耳机\u{7} ","size":bytes.len(),
            "soundMd5":hex::encode(Md5::digest(&bytes)),"deviceSoundUrl":URL}]});
        let mut replies = vec![
            json!({"code":200,"songs":[{"id":1,"name":"Synthetic","dt":30000,"ar":[],"al":{"id":1,"name":"Synthetic"}}],"privileges":[]}).into(),
            json!({"code":200,"data":[{"id":1,"code":200,"url":"https://m1.music.126.net/fixture.mp3","md5":"abcd","expi":10,"level":"standard"}]}).into(),
            model_row.clone().into(),
        ];
        if cancel_before_load {
            replies.push(model_row.into());
        }
        let api = Arc::new(ScriptApi::new(replies));
        let session = world(api.clone(), registry.store());
        let epoch = session.token().unwrap().generation.to_string();
        let device = Arc::new(playback_support::Device::default());
        device.hold_load.store(true, Ordering::SeqCst);
        let audio = device.clone();
        // In the non-cancel scenario the engine explicitly rejects installation.
        let client = MusicClient::with_session(session, registry)
            .with_effect_material_backend(Arc::new(Origin::new(bytes)))
            .with_audio_factory(move || Ok(playback_support::audio_engine(audio.clone())));
        let brand = EffectId::new(7002).unwrap();
        let load = client.play_selection(
            &epoch,
            PlaySelection::Songs {
                ids: vec!["1".into()],
                index: 0,
                quality: Quality::Standard,
            },
        );
        let tune = async {
            device.started.notified().await;
            assert_eq!(
                client
                    .apply_native_device_parametric(&epoch, brand, id())
                    .await
                    .unwrap(),
                ParametricApplication::DeferredUntilLoaded
            );
            let pending = client
                .player_snapshot(&epoch)
                .await
                .unwrap()
                .device_parametric
                .unwrap();
            assert_eq!(pending.status, ParametricBindingStatus::Deferred);
            assert_eq!(pending.model_name.as_deref(), Some("合成耳机"));
            let first = pending.activation_id.simple().to_string();
            assert!(matches!(
                client
                    .clear_native_device_parametric("0", brand, id(), &first)
                    .await,
                Err(Error::StaleResponse)
            ));
            if cancel_before_load {
                assert_eq!(
                    client
                        .clear_native_device_parametric(&epoch, brand, id(), &first)
                        .await
                        .unwrap(),
                    ParametricApplication::Cleared
                );
                assert_eq!(
                    client
                        .apply_native_device_parametric(&epoch, brand, id())
                        .await
                        .unwrap(),
                    ParametricApplication::DeferredUntilLoaded
                );
                let next = client
                    .player_snapshot(&epoch)
                    .await
                    .unwrap()
                    .device_parametric
                    .unwrap();
                assert_ne!(next.activation_id, pending.activation_id);
                assert!(matches!(
                    client
                        .clear_native_device_parametric(&epoch, brand, id(), &first)
                        .await,
                    Err(Error::StaleResponse)
                ));
                assert_eq!(
                    client
                        .player_snapshot(&epoch)
                        .await
                        .unwrap()
                        .device_parametric,
                    Some(next.clone())
                );
                assert_eq!(
                    client
                        .clear_native_device_parametric(
                            &epoch,
                            brand,
                            id(),
                            &next.activation_id.simple().to_string()
                        )
                        .await
                        .unwrap(),
                    ParametricApplication::Cleared
                );
            }
            device.hold_load.store(false, Ordering::SeqCst);
            pending.activation_id
        };
        let (result, activation_id) =
            tokio::time::timeout(Duration::from_secs(6), async { tokio::join!(load, tune) })
                .await
                .unwrap();
        let view = result.unwrap();
        assert_eq!(view.state, PlaybackStatus::Playing);
        assert_eq!(
            view.device_parametric.as_ref().map(|view| view.status),
            (!cancel_before_load).then_some(ParametricBindingStatus::ProcessorFailed)
        );
        if let Some(failed) = view.device_parametric {
            assert_eq!(failed.activation_id, activation_id);
            assert_eq!(failed.model_name.as_deref(), Some("合成耳机"));
        }
        assert_eq!(*device.parametric_at_unpause.lock().unwrap(), [false]);
        assert_eq!(api.calls().len(), if cancel_before_load { 4 } else { 3 });
    }
}

#[test]
fn five_hundred_twelve_official_authorization_decisions() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/effects-rights-official.json")).unwrap();
    let rows = fixture["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 512);
    for (index, row) in rows.iter().enumerate() {
        let host = HostRights {
            anonymous: row["host"]["anonymous"].as_bool().unwrap(),
            vinyl_vip: row["host"]["vinyl_vip"].as_bool().unwrap(),
            plus_vip: row["host"]["plus_vip"].as_bool().unwrap(),
        };
        let catalogue = Authorization::from_official(&row["catalogue"]);
        let queried = Authorization::from_official(&row["query"]["data"]);
        let query = if row["query"]["status"] == "success" {
            AuthQuery::Confirmed(queried.as_ref())
        } else {
            AuthQuery::Unavailable
        };
        assert_eq!(
            serde_json::to_value(decide_access(host, catalogue.as_ref(), query)).unwrap(),
            row["expected"],
            "case {index}"
        );
    }
}
#[test]
fn material_deserialization_and_first_entry_cannot_bypass_validation() {
    let (bytes, _) = sample("first");
    let official = sound(&bytes, URL);
    let valid = Sound::from_official(&official)
        .unwrap()
        .parameter_material()
        .unwrap()
        .unwrap();
    let stored = serde_json::to_value(&valid).unwrap();
    let restored: MaterialDescriptor = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(restored.md5(), valid.md5());
    for (key, value) in [
        ("size", Value::Null),
        ("size", json!(0)),
        ("size", json!(64 * 1024 * 1024 + 1)),
        ("md5", json!("bad")),
        ("md5", Value::Null),
        ("url", json!("https://localhost/secret")),
    ] {
        let mut bad = stored.clone();
        bad["resource"][key] = value;
        assert!(serde_json::from_value::<MaterialDescriptor>(bad).is_err());
    }
    let mut bad = official;
    bad["soundMaterialList"][0]["type"] = json!(2);
    bad["soundMaterialList"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":1,"link":URL,"size":bytes.len(),"md5":valid.md5()}));
    assert!(
        Sound::from_official(&bad)
            .unwrap()
            .parameter_material()
            .is_err()
    );
    for id in [0u64, 9_007_199_254_740_992] {
        assert!(serde_json::from_value::<EffectId>(json!(id)).is_err());
    }
}
#[tokio::test]
async fn official_api_paths_payloads_and_missing_auth_have_distinct_semantics() {
    let api=Arc::new(ScriptApi::new([
        json!({"code":200,"data":[{"id":300}]}).into(),json!({"code":200,"data":[]}).into(),json!({"code":200,"data":[]}).into(),
        json!({"code":200,"data":[]}).into(),json!({"code":200,"data":[]}).into(),
        json!({"code":200,"data":[{"interestsId":200,"interestsType":116,"authResp":{"authResult":{"canUse":true}}}]}).into(),
        json!({"code":200,"data":[]}).into(),json!({"code":200,"data":{}}).into(),
        json!({"code":200,"data":{"soundKey":"0"}}).into(),json!({"code":200,"data":[{"id":999,"effect":null}]}).into(),json!({"code":200,"data":[]}).into(),
    ]));
    let effects = EffectsApi::new(api.clone());
    assert_eq!(
        effects.catalogue(CatalogueKind::More).await.unwrap().len(),
        1
    );
    effects.legacy_catalogue(true).await.unwrap();
    effects.legacy_catalogue(false).await.unwrap();
    effects.device_brands().await.unwrap();
    effects
        .device_sounds(EffectId::new(9).unwrap())
        .await
        .unwrap();
    assert!(
        effects
            .authorization(EffectId::new(200).unwrap())
            .await
            .unwrap()
            .unwrap()
            .can_use
    );
    assert!(effects.authorization(id()).await.unwrap().is_none());
    effects.recycle_query(id()).await.unwrap();
    assert_eq!(effects.smart_key(&json!(123)).await.unwrap(), "0");
    assert_eq!(
        effects.song_parameters(&json!(123)).await.unwrap().unwrap()["id"],
        999
    );
    assert!(
        effects
            .emotion_features(&json!(123))
            .await
            .unwrap()
            .is_none()
    );
    let expected = [
        ("/api/usertool/sound/queryAllSound", json!({"exposeType":7})),
        ("/api/usertool/sound/mobile/promote", json!({})),
        ("/api/usertool/sound/mobile/all", json!({})),
        ("/api/usertool/home/soundeffect/devicebrand/list", json!({})),
        (
            "/api/usertool/home/soundeffect/devicesound/list",
            json!({"deviceManagerId":9}),
        ),
        (
            "/api/vipauth/app/interests/auth/query",
            json!({"queryInfo":{"interestsMap":{"116":[200]}}}),
        ),
        (
            "/api/vipauth/app/interests/auth/query",
            json!({"queryInfo":{"interestsMap":{"127":[300]}}}),
        ),
        (
            "/api/vipauth/app/interests/recyclequery",
            json!({"queryInfo":{"127":["300"]}}),
        ),
        ("/api/usertool/smart/sound/get", json!({"songId":"123"})),
        (
            "/api/vip-rights/svip/sound/effect/get",
            json!({"songIds":["123"]}),
        ),
        (
            "/api/vip-rights/emotion/audio/effect/get",
            json!({"songIds":["123"]}),
        ),
    ];
    for (call, (path, payload)) in api.calls().iter().zip(expected) {
        assert_eq!(call.path, path);
        assert_eq!(call.payload, payload);
        assert!(!call.cache);
    }
}
#[tokio::test]
async fn catalogue_failed_refresh_retains_complete_snapshot_and_switch_rejects_late_reply() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (pending, send) = gate();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":[{"id":300}]}).into(),
        json!({"code":200,"data":[{"id":300},{"id":300}]}).into(),
        pending,
    ]));
    let session = world(api.clone(), store);
    let library = EffectsLibrary::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    library
        .catalogue_for(&token, CatalogueKind::More, true)
        .await
        .unwrap();
    assert!(
        library
            .catalogue_for(&token, CatalogueKind::More, true)
            .await
            .is_err()
    );
    assert_eq!(
        library
            .catalogue_for(&token, CatalogueKind::More, false)
            .await
            .unwrap()
            .len(),
        1
    );
    let worker = {
        let l = library.clone();
        let t = token.clone();
        tokio::spawn(async move { l.catalogue_for(&t, CatalogueKind::More, true).await })
    };
    api.wait_for_calls(3).await;
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    send.send(Ok(json!({"code":200,"data":[{"id":301}]})))
        .unwrap();
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    assert_eq!(
        library
            .catalogue_for(&session.token().unwrap(), CatalogueKind::More, false)
            .await
            .unwrap()[0]
            .id,
        id()
    );
    assert_eq!(api.calls().len(), 3);
}
#[tokio::test]
async fn material_pause_reopen_renew_cached_denial_and_owned_removal() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, parameters) = sample("first");
    let first = sound(&bytes, URL);
    let renewed = sound(
        &bytes,
        "https://m.test.music.126.net/material?lease=renewed",
    );
    let api = Arc::new(ScriptApi::new(
        [
            pair(&first, true),
            pair(&renewed, true),
            pair(&renewed, true),
            pair(&renewed, false),
        ]
        .into_iter()
        .flatten(),
    ));
    let origin = Arc::new(Origin::new(bytes.clone()));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session.clone(), origin.clone()).unwrap();
    let paused = library
        .acquire_for(
            &token,
            id(),
            RunOptions {
                byte_budget: Some(17),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(paused.material.is_none());
    assert_eq!(paused.bytes, 17);
    assert_eq!(
        library.phase_for(&token, &paused.handle).unwrap(),
        Some(MaterialPhase::Pending)
    );
    drop(library);
    let reopened = Arc::new(StateStore::open(temp.path()).unwrap());
    let session = world(api.clone(), reopened.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
    let ready = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    let material = ready.material.unwrap();
    assert_eq!(material.parameters, parameters);
    assert_eq!(ready.sound.catalogue_sound_type, Some(3));
    assert_eq!(ready.transfer_id, paused.transfer_id);
    assert_eq!(std::fs::read(&material.path).unwrap(), bytes);
    assert_eq!(origin.calls()[1].offset, 17);
    assert!(origin.calls()[1].url.as_str().ends_with("lease=renewed"));
    assert!(
        library
            .acquire_for(&token, id(), RunOptions::default())
            .await
            .unwrap()
            .material
            .is_some()
    );
    assert_eq!(origin.calls().len(), 2);
    assert!(matches!(
        library
            .acquire_for(&token, id(), RunOptions::default())
            .await,
        Err(Error::State("effect material is not authorized"))
    ));
    assert_eq!(origin.calls().len(), 2);
    library.remove_for(&token, &paused.handle).unwrap();
    library.remove_for(&token, &paused.handle).unwrap();
    assert!(!material.path.exists());
    assert_eq!(
        library.phase_for(&token, &paused.handle).unwrap(),
        Some(MaterialPhase::Removed)
    );
    assert_eq!(
        transfer(reopened, origin)
            .get(&ready.transfer_id)
            .unwrap()
            .status,
        TransferStatus::Cancelled
    );
    assert_eq!(
        api.calls()[0].payload,
        json!({"reqListStr":"[{\"soundId\":\"300\",\"soundType\":10}]"})
    );
}
#[tokio::test]
async fn changed_material_digest_starts_new_file_without_overwriting_cached_one() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (a, _) = sample("first");
    let (b, p) = sample("replacement");
    let api = Arc::new(ScriptApi::new(
        [pair(&sound(&a, URL), true), pair(&sound(&b, URL), true)]
            .into_iter()
            .flatten(),
    ));
    let origin = Arc::new(Origin::new(a.clone()));
    let session = world(api, store);
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
    let first = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    origin.settings.lock().unwrap().data = b;
    let second = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    assert_ne!(first.handle, second.handle);
    assert_ne!(first.transfer_id, second.transfer_id);
    assert_eq!(origin.calls()[1].offset, 0);
    assert_eq!(second.material.unwrap().parameters, p);
    assert_eq!(std::fs::read(first.material.unwrap().path).unwrap(), a);
}
#[tokio::test]
async fn switching_account_during_body_never_publishes_old_account_result() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let s = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        [pair(&s, true), pair(&s, true)].into_iter().flatten(),
    ));
    let origin = Arc::new(Origin::new(bytes));
    let read_gate = Arc::new(ReadGate::default());
    origin.settings.lock().unwrap().gate = Some(read_gate.clone());
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session.clone(), origin.clone()).unwrap();
    let worker = {
        let l = library.clone();
        let t = token.clone();
        tokio::spawn(async move { l.acquire_for(&t, id(), RunOptions::default()).await })
    };
    read_gate.reached.notified().await;
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    read_gate.release.notify_one();
    assert!(matches!(worker.await.unwrap(), Err(Error::StaleResponse)));
    let job = transfer(store, origin.clone())
        .list()
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(job.status, TransferStatus::Paused);
    assert_eq!(job.bytes, 0);
    assert!(!job.target.exists());
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    origin.settings.lock().unwrap().gate = None;
    assert!(
        library
            .acquire_for(&session.token().unwrap(), id(), RunOptions::default())
            .await
            .unwrap()
            .material
            .is_some()
    );
}
#[tokio::test]
async fn cancelled_worker_releases_lock_and_preserves_transfer_for_explicit_resume() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let s = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        [pair(&s, true), pair(&s, true)].into_iter().flatten(),
    ));
    let origin = Arc::new(Origin::new(bytes));
    let read_gate = Arc::new(ReadGate::default());
    origin.settings.lock().unwrap().gate = Some(read_gate.clone());
    let session = world(api, store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
    let worker = {
        let l = library.clone();
        let t = token.clone();
        tokio::spawn(async move { l.acquire_for(&t, id(), RunOptions::default()).await })
    };
    read_gate.reached.notified().await;
    assert!(matches!(
        library
            .acquire_for(&token, id(), RunOptions::default())
            .await,
        Err(Error::MutationBusy)
    ));
    worker.abort();
    assert!(matches!(worker.await, Err(error) if error.is_cancelled()));
    assert_eq!(
        transfer(store, origin.clone()).list().unwrap()[0].status,
        TransferStatus::Paused
    );
    origin.settings.lock().unwrap().gate = None;
    assert!(
        library
            .acquire_for(&token, id(), RunOptions::default())
            .await
            .unwrap()
            .material
            .is_some()
    );
}
#[tokio::test]
async fn orphan_before_child_commit_is_retained_and_new_attempt_recovers() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let s = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        [pair(&s, true), pair(&s, true)].into_iter().flatten(),
    ));
    let origin = Arc::new(Origin::new(bytes));
    let session = world(api, store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
    let paused = library
        .acquire_for(
            &token,
            id(),
            RunOptions {
                byte_budget: Some(17),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let child = transfer(store.clone(), origin.clone())
        .get(&paused.transfer_id)
        .unwrap();
    let before = std::fs::read(&child.part).unwrap();
    store.delete("1", "media_transfers_v1", &child.id).unwrap();
    let recovered = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    assert_ne!(recovered.transfer_id, paused.transfer_id);
    assert!(recovered.material.is_some());
    assert_eq!(std::fs::read(child.part).unwrap(), before);
    assert_eq!(origin.calls()[1].offset, 0);
    let parent = store
        .get("1", "effects-materials-v1", &paused.handle.key)
        .unwrap()
        .unwrap();
    assert_eq!(parent["abandoned"], json!([paused.transfer_id]));
}
#[tokio::test]
async fn completed_child_before_parent_commit_recovers_without_second_download() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let s = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        [pair(&s, true), pair(&s, true)].into_iter().flatten(),
    ));
    let origin = Arc::new(Origin::new(bytes));
    let session = world(api, store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
    let paused = library
        .acquire_for(
            &token,
            id(),
            RunOptions {
                byte_budget: Some(17),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    transfer(store.clone(), origin.clone())
        .run(&paused.transfer_id, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(
        library.phase_for(&token, &paused.handle).unwrap(),
        Some(MaterialPhase::Pending)
    );
    let result = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    assert!(result.material.is_some());
    assert_eq!(origin.calls().len(), 2);
    assert_eq!(
        library.phase_for(&token, &paused.handle).unwrap(),
        Some(MaterialPhase::Ready)
    );
}
#[tokio::test]
async fn checksum_ncae_and_json_failures_never_publish_ready_parameters() {
    let (valid, _) = sample("first");
    let (array, _) = sample("array");
    for (declared, actual) in [
        (valid.clone(), vec![0; valid.len()]),
        (b"not NCAE".to_vec(), b"not NCAE".to_vec()),
        (array.clone(), array),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let api = Arc::new(ScriptApi::new(pair(&sound(&declared, URL), true)));
        let origin = Arc::new(Origin::new(actual));
        let session = world(api, store.clone());
        let token = session.token().unwrap();
        let library = EffectsLibrary::with_backend(session, origin).unwrap();
        assert!(
            library
                .acquire_for(&token, id(), RunOptions::default())
                .await
                .is_err()
        );
        let parent = store
            .values("1", "effects-materials-v1")
            .unwrap()
            .into_values()
            .next()
            .unwrap();
        assert_eq!(parent["phase"], "pending");
        assert!(parent["plain_sha256"].is_null());
    }
}
#[tokio::test]
async fn tampered_completed_material_is_rejected_and_foreign_replacement_is_never_removed() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let s = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        [pair(&s, true), pair(&s, true)].into_iter().flatten(),
    ));
    let origin = Arc::new(Origin::new(bytes));
    let session = world(api, store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin).unwrap();
    let ready = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    let path = ready.material.unwrap().path;
    let foreign = path.with_extension("foreign");
    std::fs::write(&foreign, b"user file").unwrap();
    std::fs::rename(&path, path.with_extension("old")).unwrap();
    std::fs::rename(&foreign, &path).unwrap();
    assert!(
        library
            .acquire_for(&token, id(), RunOptions::default())
            .await
            .is_err()
    );
    assert!(library.remove_for(&token, &ready.handle).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"user file");
    assert_eq!(
        library.phase_for(&token, &ready.handle).unwrap(),
        Some(MaterialPhase::Removing)
    );
}
#[tokio::test]
async fn removal_intent_recovers_and_other_accounts_cannot_use_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let s = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(pair(&s, true)));
    let origin = Arc::new(Origin::new(bytes));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session.clone(), origin.clone()).unwrap();
    let ready = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    let path = ready.material.unwrap().path;
    let mut parent = store
        .get("1", "effects-materials-v1", &ready.handle.key)
        .unwrap()
        .unwrap();
    parent["phase"] = json!("removing");
    store
        .put("1", "effects-materials-v1", &ready.handle.key, &parent)
        .unwrap();
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    assert!(
        library
            .remove_for(&session.token().unwrap(), &ready.handle)
            .is_err()
    );
    assert!(path.exists());
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    library
        .remove_for(&session.token().unwrap(), &ready.handle)
        .unwrap();
    assert!(!path.exists());
    // Crash after child deletion but before the parent tombstone commit.
    store
        .put("1", "effects-materials-v1", &ready.handle.key, &parent)
        .unwrap();
    library
        .remove_for(&session.token().unwrap(), &ready.handle)
        .unwrap();
    assert_eq!(
        library
            .phase_for(&session.token().unwrap(), &ready.handle)
            .unwrap(),
        Some(MaterialPhase::Removed)
    );
}

#[tokio::test]
async fn cached_vip_marker_never_grants_download_when_fresh_auth_is_denied_missing_or_broken() {
    let (bytes, _) = sample("first");
    let sound = sound(&bytes, URL);
    let allowed = json!({"interestsId":300,"authResp":{"authResult":{"canUse":true}}});
    for reply in [
        auth(false),
        json!({"code":200,"data":[]}).into(),
        json!({"code":200,"data":[allowed.clone(),allowed]}).into(),
        json!({"code":200,"data":{}}).into(),
        json!({"code":403}).into(),
        Reply::Ready(Err(Error::Transport("synthetic failure".into()))),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let api = Arc::new(ScriptApi::new([detail(&sound), reply]));
        let origin = Arc::new(Origin::new(bytes.clone()));
        let session = world(api.clone(), store.clone());
        let token = session.token().unwrap();
        let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
        assert!(
            library
                .acquire_for(&token, id(), RunOptions::default())
                .await
                .is_err()
        );
        assert!(origin.calls().is_empty());
        assert!(
            store
                .values("1", "effects-materials-v1")
                .unwrap()
                .is_empty()
        );
        assert_eq!(api.calls().len(), 2);
    }
}

#[tokio::test]
async fn cached_material_still_honors_caller_cancellation_and_pause() {
    use cumulus_core::download::transfer::TransferDirective;
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let sound = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new((0..3).flat_map(|_| pair(&sound, true))));
    let origin = Arc::new(Origin::new(bytes));
    let session = world(api, store);
    let token = session.token().unwrap();
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();
    library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    let cancelled = library
        .acquire_for(
            &token,
            id(),
            RunOptions {
                control: Arc::new(|| Err(Error::Cancelled)),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(cancelled, Err(Error::Cancelled)));
    let paused = library
        .acquire_for(
            &token,
            id(),
            RunOptions {
                control: Arc::new(|| Ok(TransferDirective::Pause)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(paused.material.is_none());
    assert_eq!(origin.calls().len(), 1);
    assert_eq!(
        library.phase_for(&token, &paused.handle).unwrap(),
        Some(MaterialPhase::Pending)
    );
}

#[tokio::test]
async fn prepared_parametric_comes_only_from_verified_owned_material_and_never_registers() {
    use cumulus_core::effects::service::ParametricPreparation;
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let official = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        [pair(&official, true), pair(&official, true)]
            .into_iter()
            .flatten(),
    ));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    assert!(matches!(
        library
            .prepare_parametric_for(
                &token,
                id(),
                RunOptions {
                    byte_budget: Some(17),
                    ..Default::default()
                }
            )
            .await
            .unwrap(),
        ParametricPreparation::Pending { bytes: 17 }
    ));
    let ParametricPreparation::Prepared(prepared) = library
        .prepare_parametric_for(&token, id(), RunOptions::default())
        .await
        .unwrap()
    else {
        panic!("verified official branch fixture must expose its PEQ component");
    };
    assert_eq!(prepared.effect(), id());
    assert!(prepared.enabled());
    assert!(prepared.additional_sections().is_empty());
    assert!(
        library
            .prepared_parametric_for(&token, &prepared)
            .unwrap()
            .enabled()
    );
    let same_uid_session = world(api.clone(), store.clone());
    assert_eq!(same_uid_session.token().unwrap(), token);
    let other_library =
        EffectsLibrary::with_backend(same_uid_session, Arc::new(Origin::new(Vec::new()))).unwrap();
    assert!(matches!(
        other_library.prepared_parametric_for(&token, &prepared),
        Err(Error::StaleResponse)
    ));

    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    assert!(matches!(
        library.prepared_parametric_for(&session.token().unwrap(), &prepared),
        Err(Error::StaleResponse)
    ));
    assert!(
        library
            .prepare_parametric_for(
                &session.token().unwrap(),
                EffectId::new(200).unwrap(),
                RunOptions::default()
            )
            .await
            .is_err()
    );
    assert_eq!(
        api.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/usertool/home/sound/info",
            "/api/vipauth/app/interests/auth/query",
            "/api/usertool/home/sound/info",
            "/api/vipauth/app/interests/auth/query",
        ]
    );
}

#[tokio::test]
async fn standalone_activation_preflight_rejects_unproven_rights_before_transfer() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let official = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new([
        detail(&official),
        json!({"code":200,"data":[{"interestsId":300,"interestsType":127,
            "authResp":{"authResult":{"canUse":true,"canUseType":50}}}]})
        .into(),
    ]));
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let origin = Arc::new(Origin::new(bytes));
    let library = EffectsLibrary::with_backend(session, origin.clone()).unwrap();

    assert!(matches!(
        library
            .preflight_parametric_activation_for(&token, id(), RunOptions::default())
            .await,
        Err(Error::State(
            "effect material requires confirmed non-trial rights"
        ))
    ));
    assert_eq!(api.calls().len(), 2);
    assert!(origin.calls().is_empty());
}

#[tokio::test]
async fn standalone_activation_preflight_returns_only_enabled_receipts_owned_by_the_session() {
    use cumulus_core::effects::service::StandaloneParametricPreflight;
    for name in ["first", "replacement"] {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let (bytes, _) = sample(name);
        let official = sound(&bytes, URL);
        let api = Arc::new(ScriptApi::new(pair(&official, true)));
        let session = world(api.clone(), store.clone());
        let token = session.token().unwrap();
        let library =
            EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
        let result = library
            .preflight_parametric_activation_for(&token, id(), RunOptions::default())
            .await
            .unwrap();
        if name == "first" {
            let StandaloneParametricPreflight::Ready(receipt) = result else {
                panic!("verified standalone PEQ must be eligible for the next native step");
            };
            assert_eq!(receipt.effect(), id());
            library
                .validate_parametric_preflight_for(&token, &receipt)
                .unwrap();
            let replacement = world(api.clone(), store);
            let other =
                EffectsLibrary::with_backend(replacement, Arc::new(Origin::new(Vec::new())))
                    .unwrap();
            assert!(matches!(
                other.validate_parametric_preflight_for(&token, &receipt),
                Err(Error::StaleResponse)
            ));
        } else {
            assert!(matches!(result, StandaloneParametricPreflight::Unsupported));
        }
        assert_eq!(
            api.calls()
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            [
                "/api/usertool/home/sound/info",
                "/api/vipauth/app/interests/auth/query"
            ]
        );
    }
}

#[tokio::test]
async fn preflighted_registration_seals_its_exact_observation_or_leaves_an_uncertain_lease() {
    use cumulus_core::effects::{
        authorization::{EffectAuthorizations, LeasePhase},
        service::StandaloneParametricPreflight,
    };
    for lost in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(temp.path()).unwrap());
        let (bytes, _) = sample("first");
        let official = sound(&bytes, URL);
        let mut replies = Vec::from(pair(&official, true));
        replies.push(auth(true));
        if lost {
            replies.push(Reply::Ready(Err(Error::Transport(
                "lost start receipt".into(),
            ))));
        } else {
            replies.extend([
                json!({"code":200,"data":true}).into(),
                auth(true),
                nontrial_recycle(false),
            ]);
        }
        let api = Arc::new(ScriptApi::new(replies));
        let session = world(api.clone(), store);
        let token = session.token().unwrap();
        let library =
            EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
        let authorization = EffectAuthorizations::new(session).unwrap();
        let StandaloneParametricPreflight::Ready(receipt) = library
            .preflight_parametric_activation_for(&token, id(), RunOptions::default())
            .await
            .unwrap()
        else {
            panic!("standalone fixture must be eligible");
        };

        let bound = authorization
            .register_preflighted_parametric_for(&library, &token, *receipt)
            .await;
        let lease = authorization.record_for(&token, id()).unwrap().unwrap();
        if lost {
            assert!(matches!(bound, Err(Error::MutationUncertain)));
            assert_eq!(lease.phase, LeasePhase::StartUncertain);
            assert!(!lease.start_acknowledged);
            assert_eq!(api.calls().len(), 4);
        } else {
            assert_eq!(lease.phase, LeasePhase::Registered);
            assert!(lease.start_acknowledged);
            authorization
                .validate_bound_parametric_for(&library, &token, &bound.unwrap())
                .unwrap();
            assert_eq!(api.calls().len(), 6);
        }
        let calls = api.calls();
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.path.ends_with("/set"))
                .count(),
            1
        );
        assert!(!calls.iter().any(|call| call.path.ends_with("/stop")));
    }
}

#[tokio::test]
async fn rebind_keeps_the_registration_and_rejects_its_old_revision_without_network() {
    use cumulus_core::effects::{
        authorization::{EffectAuthorizations, LeasePhase},
        service::StandaloneParametricPreflight,
    };
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let official = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        pair(&official, true)
            .into_iter()
            .chain([
                auth(true),
                json!({"code":200,"data":true}).into(),
                auth(true),
                nontrial_recycle(false),
            ])
            .chain(pair(&official, true))
            .chain([auth(true), nontrial_recycle(false)])
            .chain(pair(&official, true)),
    ));
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let authorization = EffectAuthorizations::new(session).unwrap();
    let preflight = || async {
        let StandaloneParametricPreflight::Ready(receipt) = library
            .preflight_parametric_activation_for(&token, id(), RunOptions::default())
            .await
            .unwrap()
        else {
            panic!("standalone fixture must be eligible");
        };
        receipt
    };
    authorization
        .register_preflighted_parametric_for(&library, &token, *preflight().await)
        .await
        .unwrap();
    let old = authorization.record_for(&token, id()).unwrap().unwrap();
    let existing = authorization
        .native_release_receipt_for(&token, id())
        .unwrap()
        .unwrap();
    let rebound = authorization
        .bind_preflighted_registration_for(&library, &token, *preflight().await, &existing)
        .await
        .unwrap();
    let current = authorization.record_for(&token, id()).unwrap().unwrap();
    assert_eq!(current.phase, LeasePhase::Registered);
    assert_eq!(current.operation, old.operation);
    assert!(current.revision > old.revision && current.start_acknowledged);
    authorization
        .validate_bound_parametric_for(&library, &token, &rebound)
        .unwrap();

    let stale = *preflight().await;
    let before = api.calls().len();
    assert!(matches!(
        authorization
            .bind_preflighted_registration_for(&library, &token, stale, &existing)
            .await,
        Err(Error::StaleResponse)
    ));
    let calls = api.calls();
    assert_eq!(calls.len(), before);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.path.ends_with("/set"))
            .count(),
        1
    );
    assert!(!calls.iter().any(|call| call.path.ends_with("/stop")));
}

#[tokio::test]
async fn removed_material_during_registration_read_never_records_or_sends_start() {
    use cumulus_core::effects::{
        authorization::EffectAuthorizations, service::StandaloneParametricPreflight,
    };
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let official = sound(&bytes, URL);
    let (reply, release) = gate();
    let api = Arc::new(ScriptApi::new(
        pair(&official, true)
            .into_iter()
            .chain(pair(&official, true))
            .chain([reply]),
    ));
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let authorization = EffectAuthorizations::new(session).unwrap();
    let material = library
        .acquire_for(&token, id(), RunOptions::default())
        .await
        .unwrap();
    let StandaloneParametricPreflight::Ready(receipt) = library
        .preflight_parametric_activation_for(&token, id(), RunOptions::default())
        .await
        .unwrap()
    else {
        panic!("standalone fixture must be eligible");
    };
    let pending = {
        let (authorization, library, token) =
            (authorization.clone(), library.clone(), token.clone());
        tokio::spawn(async move {
            authorization
                .register_preflighted_parametric_for(&library, &token, *receipt)
                .await
        })
    };
    api.wait_for_calls(5).await;
    library.remove_for(&token, &material.handle).unwrap();
    let Reply::Ready(auth_result) = auth(true) else {
        unreachable!()
    };
    release.send(auth_result).unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(Error::State(
            "prepared effect material is no longer available"
        ))
    ));
    assert!(authorization.record_for(&token, id()).unwrap().is_none());
    assert_eq!(api.calls().len(), 5);
}

fn nontrial_recycle(value: bool) -> Reply {
    json!({"code":200,"data":{"127":[{"interestsId":300,"canRecycle":value}]}}).into()
}

#[tokio::test]
async fn ready_peq_binding_needs_existing_registration_and_is_revoked_by_new_observation() {
    use cumulus_core::effects::{
        authorization::{EffectAuthorizations, TrialConsent},
        service::ParametricPreparation,
    };
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let api = Arc::new(ScriptApi::new(
        pair(&sound(&bytes, URL), true).into_iter().chain([
            auth(true),
            json!({"code":200,"data":true}).into(),
            auth(true),
            nontrial_recycle(false),
            auth(true),
            nontrial_recycle(false),
            auth(true),
            nontrial_recycle(true),
        ]),
    ));
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let authorizations = EffectAuthorizations::new(session).unwrap();
    let ParametricPreparation::Prepared(prepared) = library
        .prepare_parametric_for(&token, id(), RunOptions::default())
        .await
        .unwrap()
    else {
        panic!("fixture has enabled PEQ");
    };
    // A successful material query is insufficient. Only this explicit
    // fixture command may make the one synthetic registration write.
    assert!(authorizations.record_for(&token, id()).unwrap().is_none());
    authorizations
        .begin_for(&token, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    let binding = authorizations
        .bind_parametric_for(&library, &token, prepared)
        .await
        .unwrap();
    assert_eq!(binding.effect(), id());
    authorizations
        .validate_bound_parametric_for(&library, &token, &binding)
        .unwrap();
    let revoked = authorizations.observe_for(&token, id()).await.unwrap();
    assert!(!revoked.can_use);
    assert!(matches!(
        authorizations.validate_bound_parametric_for(&library, &token, &binding),
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        api.calls()
            .into_iter()
            .filter(|call| call.path.ends_with("/set") || call.path.ends_with("/stop"))
            .map(|call| call.path)
            .collect::<Vec<_>>(),
        ["/api/vipauth/app/interests/auth/set"]
    );
}

#[tokio::test]
async fn old_unclassified_registration_cannot_be_promoted_from_current_rights() {
    use cumulus_core::effects::{
        authorization::{EffectAuthorizations, TrialConsent},
        service::ParametricPreparation,
    };
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let api = Arc::new(ScriptApi::new(
        pair(&sound(&bytes, URL), true).into_iter().chain([
            auth(true),
            json!({"code":200,"data":true}).into(),
            auth(true),
            nontrial_recycle(false),
        ]),
    ));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let authorizations = EffectAuthorizations::new(session).unwrap();
    let ParametricPreparation::Prepared(prepared) = library
        .prepare_parametric_for(&token, id(), RunOptions::default())
        .await
        .unwrap()
    else {
        panic!("fixture has enabled PEQ");
    };
    authorizations
        .begin_for(&token, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    let journal = "effect-authorizations-v1";
    let mut old = store.get("1", journal, "300").unwrap().unwrap();
    old.as_object_mut().unwrap().remove("registration_use_type");
    store.put("1", journal, "300", &old).unwrap();
    let calls = api.calls().len();
    assert!(
        authorizations
            .bind_parametric_for(&library, &token, prepared)
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), calls);
}

#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback_support;

#[tokio::test]
async fn active_peq_observation_keeps_playing_during_reads_and_detaches_on_remote_revocation() {
    use cumulus_core::{
        effects::{
            authorization::{
                ActiveParametricObservation as Observation, EffectAuthorizations, TrialConsent,
            },
            service::ParametricPreparation,
        },
        playback::{
            controller::{ParametricApplication, PlaybackController, PlaybackStatus},
            source::SourceResolver,
        },
    };
    use std::{sync::atomic::Ordering, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let (held, release) = gate();
    let api = Arc::new(ScriptApi::new(pair(&sound(&bytes, URL), true).into_iter().chain([
        auth(true),
        json!({"code":200,"data":true}).into(),
        auth(true),
        nontrial_recycle(false),
        auth(true),
        nontrial_recycle(false),
        json!({"code":200,"data":[{"id":1,"code":200,"url":"https://m1.music.126.net/fixture.mp3","md5":"abcd","expi":10,"level":"standard"}]}).into(),
        held,
        nontrial_recycle(false),
        auth(true),
        nontrial_recycle(true),
    ])));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let authorizations = EffectAuthorizations::new(session.clone()).unwrap();
    let ParametricPreparation::Prepared(prepared) = library
        .prepare_parametric_for(&token, id(), RunOptions::default())
        .await
        .unwrap()
    else {
        panic!("fixture has standalone PEQ");
    };
    authorizations
        .begin_for(&token, id(), TrialConsent::DoNotStart)
        .await
        .unwrap();
    let binding = authorizations
        .bind_parametric_for(&library, &token, prepared)
        .await
        .unwrap();
    let device = Arc::new(playback_support::Device::default());
    device.parametric_supported.store(true, Ordering::SeqCst);
    let player = Arc::new(
        PlaybackController::start(
            session.clone(),
            store,
            Arc::new(SourceResolver::new(session, None)),
            playback_support::audio_engine(device.clone()),
        )
        .unwrap(),
    );

    assert_eq!(
        player
            .command(&token, playback_support::replace(&[1]))
            .await
            .unwrap()
            .state,
        PlaybackStatus::Playing
    );
    assert_eq!(
        player
            .apply_bound_parametric(&token, binding)
            .await
            .unwrap(),
        ParametricApplication::Configured
    );
    assert!(device.parametric_attached.load(Ordering::SeqCst));
    let before = device.state.lock().unwrap().stops;
    let revision = authorizations
        .record_for(&token, id())
        .unwrap()
        .unwrap()
        .revision;
    let observing = tokio::spawn({
        let (player, token) = (player.clone(), token.clone());
        async move { player.observe_active_parametric(&token).await }
    });
    api.wait_for_calls(10).await;
    assert!(matches!(
        player.observe_active_parametric(&token).await,
        Err(Error::MutationBusy)
    ));
    assert_eq!(
        player
            .command(
                &token,
                cumulus_core::playback::controller::PlaybackCommand::Snapshot
            )
            .await
            .unwrap()
            .state,
        PlaybackStatus::Playing
    );
    let Reply::Ready(positive) = auth(true) else {
        unreachable!()
    };
    release.send(positive).unwrap();
    assert_eq!(observing.await.unwrap().unwrap(), Observation::Confirmed);
    assert_eq!(
        authorizations
            .record_for(&token, id())
            .unwrap()
            .unwrap()
            .revision,
        revision
    );
    assert!(device.parametric_attached.load(Ordering::SeqCst));
    assert_eq!(
        player.observe_active_parametric(&token).await.unwrap(),
        Observation::Revoked
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while device.parametric_attached.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(player.snapshot().state, PlaybackStatus::Playing);
    assert_eq!(device.state.lock().unwrap().stops, before);
    assert_eq!(
        player.observe_active_parametric(&token).await.unwrap(),
        Observation::NoActiveBinding
    );
    let lease = authorizations.record_for(&token, id()).unwrap().unwrap();
    assert_eq!(lease.revision, revision + 1);
    assert_eq!(lease.recyclable, Some(true));
    assert_eq!(api.calls().len(), 13);
    assert_eq!(
        api.calls()
            .iter()
            .filter(|call| call.path.ends_with("/set") || call.path.ends_with("/stop"))
            .count(),
        1
    );
}

#[tokio::test]
async fn bound_parametric_clear_cannot_remove_a_newer_binding_or_release_rights() {
    exercise_bound_parametric_clear(ClearScenario::Explicit).await;
}

#[tokio::test]
async fn failed_bound_detach_retains_its_target_and_exclusive_use_fence() {
    exercise_bound_parametric_clear(ClearScenario::FailedExplicit).await;
}

#[tokio::test]
async fn deferred_bound_parametric_can_be_paused_before_the_decoder_returns() {
    exercise_bound_parametric_clear(ClearScenario::Deferred).await;
}

#[tokio::test]
async fn runtime_peq_failure_continues_same_entry_after_confirmed_detach() {
    exercise_bound_parametric_clear(ClearScenario::Processor).await;
}

#[tokio::test]
async fn runtime_peq_failure_stops_if_the_failed_node_cannot_be_removed() {
    exercise_bound_parametric_clear(ClearScenario::FailedProcessorDetach).await;
}

#[tokio::test]
async fn runtime_peq_failure_after_unpause_resamples_without_replaying_transport() {
    exercise_bound_parametric_clear(ClearScenario::ProcessorAfterUnpause).await;
}

#[derive(PartialEq, Eq)]
enum ClearScenario {
    Explicit,
    FailedExplicit,
    Deferred,
    Processor,
    FailedProcessorDetach,
    ProcessorAfterUnpause,
}

async fn exercise_bound_parametric_clear(scenario: ClearScenario) {
    use cumulus_core::{
        effects::{
            authorization::{EffectAuthorizations, TrialConsent},
            service::ParametricPreparation,
        },
        playback::{
            controller::{
                ParametricApplication, ParametricBindingStatus, PlaybackCommand,
                PlaybackController, PlaybackStatus,
            },
            source::SourceResolver,
        },
    };
    use std::sync::atomic::Ordering;

    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(temp.path()).unwrap());
    let (bytes, _) = sample("first");
    let detail = sound(&bytes, URL);
    let api = Arc::new(ScriptApi::new(
        pair(&detail, true)
            .into_iter()
            .chain(pair(&detail, true))
            .chain([
                auth(true),
                json!({"code":200,"data":true}).into(),
                auth(true),
                nontrial_recycle(false),
                auth(true),
                nontrial_recycle(false),
                auth(true),
                nontrial_recycle(false),
                json!({"code":200,"data":[{"id":1,"code":200,"url":"https://m1.music.126.net/fixture.mp3","md5":"abcd","expi":10,"level":"standard"}]}).into(),
            ]),
    ));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let mut materials = Vec::new();
    for _ in 0..2 {
        let ParametricPreparation::Prepared(prepared) = library
            .prepare_parametric_for(&token, id(), RunOptions::default())
            .await
            .unwrap()
        else {
            panic!("fixture has standalone PEQ");
        };
        materials.push(prepared);
    }
    let authorization = EffectAuthorizations::new(session.clone()).unwrap();
    authorization
        .begin_for(&token, id(), TrialConsent::KnownNonTrial)
        .await
        .unwrap();
    let mut bindings = Vec::new();
    for prepared in materials {
        bindings.push(
            authorization
                .bind_parametric_for(&library, &token, prepared)
                .await
                .unwrap(),
        );
    }
    let old = bindings[0].target();
    let binding = bindings.pop().unwrap();
    let target = binding.target();
    let device = Arc::new(playback_support::Device::default());
    device.parametric_supported.store(true, Ordering::SeqCst);
    let player = PlaybackController::start(
        session.clone(),
        store,
        Arc::new(SourceResolver::new(session, None)),
        playback_support::audio_engine(device.clone()),
    )
    .unwrap();
    if scenario == ClearScenario::Deferred {
        use std::time::Duration;
        device.hold_load.store(true, Ordering::SeqCst);
        let loading = player.command(&token, playback_support::replace(&[1]));
        let pause = async {
            tokio::time::timeout(Duration::from_secs(4), device.started.notified())
                .await
                .unwrap();
            assert_eq!(
                player
                    .apply_bound_parametric(&token, binding)
                    .await
                    .unwrap(),
                ParametricApplication::DeferredUntilLoaded
            );
            assert_eq!(
                player
                    .inspect_bound_parametric(&token, target)
                    .await
                    .unwrap(),
                ParametricBindingStatus::Deferred
            );
            let calls = api.calls().len();
            assert_eq!(
                player.clear_bound_parametric(&token, target).await.unwrap(),
                ParametricApplication::Cleared
            );
            assert_eq!(
                player
                    .inspect_bound_parametric(&token, target)
                    .await
                    .unwrap(),
                ParametricBindingStatus::Absent
            );
            assert_eq!(api.calls().len(), calls);
            assert!(authorization.acquire_bound_use_fence_for(&token).is_ok());
            device.hold_load.store(false, Ordering::SeqCst);
        };
        let (playback, ()) = tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(loading, pause)
        })
        .await
        .unwrap();
        assert_eq!(playback.unwrap().state, PlaybackStatus::Playing);
        assert_eq!(*device.parametric_at_unpause.lock().unwrap(), [false]);
        return;
    }
    let playing = player
        .command(&token, playback_support::replace(&[1]))
        .await
        .unwrap();
    assert_eq!(playing.state, PlaybackStatus::Playing);
    assert_eq!(
        player
            .apply_bound_parametric(&token, binding)
            .await
            .unwrap(),
        ParametricApplication::Configured,
    );
    assert!(matches!(
        player.clear_bound_parametric(&token, old).await,
        Err(Error::StaleResponse)
    ));
    assert!(device.parametric_attached.load(Ordering::SeqCst));
    assert!(matches!(
        authorization.acquire_bound_use_fence_for(&token),
        Err(Error::MutationBusy)
    ));
    let calls = api.calls().len();
    if matches!(
        scenario,
        ClearScenario::Processor
            | ClearScenario::FailedProcessorDetach
            | ClearScenario::ProcessorAfterUnpause
    ) {
        let cannot_detach = scenario == ClearScenario::FailedProcessorDetach;
        let during_unpause = scenario == ClearScenario::ProcessorAfterUnpause;
        if during_unpause {
            let paused = player
                .command(&token, PlaybackCommand::Pause(true))
                .await
                .unwrap();
            assert_eq!(paused.state, PlaybackStatus::Paused);
        }
        device
            .parametric_clear_fail
            .store(cannot_detach, Ordering::SeqCst);
        let native_stops = device.state.lock().unwrap().stops;
        let native_pauses = device.state.lock().unwrap().pauses.len();
        device.parametric_poll_fault.store(true, Ordering::SeqCst);
        let after = if during_unpause {
            let snapshot = player
                .command(&token, PlaybackCommand::Pause(false))
                .await
                .unwrap();
            assert_eq!(
                snapshot.bound_parametric,
                Some(ParametricBindingStatus::ProcessorFailed)
            );
            assert_eq!(device.state.lock().unwrap().pauses.len(), native_pauses + 1);
            snapshot
        } else {
            playback_support::until(&player, |snapshot| {
                if cannot_detach {
                    snapshot.state == PlaybackStatus::Error
                } else {
                    snapshot.bound_parametric == Some(ParametricBindingStatus::ProcessorFailed)
                }
            })
            .await
        };
        assert_eq!(api.calls().len(), calls);
        assert!(
            authorization
                .record_for(&token, id())
                .unwrap()
                .unwrap()
                .start_acknowledged
        );
        if cannot_detach {
            assert_eq!(
                after.bound_parametric,
                Some(ParametricBindingStatus::DetachRequired)
            );
            assert!(device.state.lock().unwrap().stops > native_stops);
            assert!(authorization.acquire_bound_use_fence_for(&token).is_err());
            device.parametric_clear_fail.store(false, Ordering::SeqCst);
        } else {
            assert_eq!(after.state, PlaybackStatus::Playing);
            assert_eq!(after.play_id, playing.play_id);
            assert_eq!(device.state.lock().unwrap().stops, native_stops);
            assert!(!device.parametric_attached.load(Ordering::SeqCst));
            assert_eq!(
                player
                    .inspect_bound_parametric(&token, target)
                    .await
                    .unwrap(),
                ParametricBindingStatus::ProcessorFailed
            );
            assert!(authorization.acquire_bound_use_fence_for(&token).is_ok());
            device.state.lock().unwrap().view.position = Some(4.0);
            let advanced =
                playback_support::until(&player, |snapshot| snapshot.position == Some(4.0)).await;
            assert_eq!(advanced.play_id, playing.play_id);
            assert_eq!(
                player.clear_bound_parametric(&token, target).await.unwrap(),
                ParametricApplication::Cleared
            );
            assert_eq!(
                player
                    .inspect_bound_parametric(&token, target)
                    .await
                    .unwrap(),
                ParametricBindingStatus::Absent
            );
        }
        return;
    }
    if scenario == ClearScenario::FailedExplicit {
        device.parametric_clear_fail.store(true, Ordering::SeqCst);
        assert!(player.clear_bound_parametric(&token, target).await.is_err());
        assert!(device.parametric_attached.load(Ordering::SeqCst));
        assert!(matches!(
            authorization.acquire_bound_use_fence_for(&token),
            Err(Error::MutationBusy)
        ));
        device.parametric_clear_fail.store(false, Ordering::SeqCst);
    }
    assert_eq!(
        player.clear_bound_parametric(&token, target).await.unwrap(),
        ParametricApplication::Cleared,
    );
    assert!(!device.parametric_attached.load(Ordering::SeqCst));
    assert!(authorization.acquire_bound_use_fence_for(&token).is_ok());
    if scenario == ClearScenario::Explicit {
        assert_eq!(player.snapshot().play_id, playing.play_id);
        assert_eq!(player.snapshot().state, PlaybackStatus::Playing);
    }
    assert_eq!(api.calls().len(), calls);
    assert!(
        authorization
            .record_for(&token, id())
            .unwrap()
            .unwrap()
            .start_acknowledged
    );
}

#[tokio::test]
async fn native_activation_consumes_the_receipt_and_submits_the_exact_binding() {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, NativeParametricActivation, NativeParametricActivationAttempt},
        effects::{authorization::LeasePhase, service::StandaloneParametricPreflight},
        playback::controller::{ParametricApplication, ParametricBindingStatus},
        protocol::http::ApiOrigin,
    };
    use std::sync::atomic::Ordering;

    let temp = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(temp.path(), ApiOrigin::Pc).unwrap();
    let (bytes, _) = sample("first");
    let api = Arc::new(ScriptApi::new(
        pair(&sound(&bytes, URL), true).into_iter().chain([
            auth(true),
            json!({"code":200,"data":true}).into(),
            auth(true),
            nontrial_recycle(false),
        ]),
    ));
    let session = world(api.clone(), registry.store());
    let token = session.token().unwrap();
    let epoch = token.generation.to_string();
    let library =
        EffectsLibrary::with_backend(session.clone(), Arc::new(Origin::new(bytes))).unwrap();
    let StandaloneParametricPreflight::Ready(receipt) = library
        .preflight_parametric_activation_for(&token, id(), RunOptions::default())
        .await
        .unwrap()
    else {
        panic!("fixture must have standalone PEQ");
    };
    let device = Arc::new(playback_support::Device::default());
    device.parametric_supported.store(true, Ordering::SeqCst);
    let audio = device.clone();
    let client = MusicClient::with_session(session, registry)
        .with_audio_factory(move || Ok(playback_support::audio_engine(audio.clone())));
    let mut attempt = NativeParametricActivationAttempt::new(receipt);
    let NativeParametricActivation::Configured(target) = client
        .activate_native_effect_parametric(&epoch, &mut attempt)
        .await
        .unwrap()
    else {
        panic!("idle engine should acknowledge configuration, not PCM");
    };
    assert_eq!(attempt.submitted_target(), Some(target));
    assert_eq!(
        client
            .inspect_native_bound_parametric(&epoch, target)
            .await
            .unwrap(),
        ParametricBindingStatus::Configured
    );
    assert!(device.parametric_attached.load(Ordering::SeqCst));
    assert_eq!(
        client
            .player_snapshot(&epoch)
            .await
            .unwrap()
            .bound_parametric,
        Some(ParametricBindingStatus::Configured)
    );
    assert_eq!(
        client
            .native_effect_registration_phase(&epoch, id())
            .unwrap(),
        Some(LeasePhase::Registered)
    );
    assert!(
        client
            .activate_native_effect_parametric(&epoch, &mut attempt)
            .await
            .is_err()
    );
    assert_eq!(
        client
            .clear_native_bound_parametric(&epoch, target)
            .await
            .unwrap(),
        ParametricApplication::Cleared
    );
    assert!(!device.parametric_attached.load(Ordering::SeqCst));
    assert_eq!(
        client
            .inspect_native_bound_parametric(&epoch, target)
            .await
            .unwrap(),
        ParametricBindingStatus::Absent
    );
    assert!(
        client
            .player_snapshot(&epoch)
            .await
            .unwrap()
            .bound_parametric
            .is_none()
    );
    assert_eq!(api.calls().len(), 6);
}
