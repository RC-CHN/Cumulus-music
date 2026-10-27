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
