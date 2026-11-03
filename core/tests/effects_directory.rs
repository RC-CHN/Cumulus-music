mod support;

use cumulus_core::{
    Error,
    account::AccountSession,
    effects::{
        catalog::{CatalogueKind, EffectId},
        directory::EffectDirectoryAccess,
        service::EffectsLibrary,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{ScriptApi, gate};

fn sound() -> Value {
    json!({"id":200,"name":"Synthetic","subTitle":"Spatial","soundType":3,
        "coverUrl":"https://example.invalid/secret",
        "lightImg":"http://p1.music.126.net/cover.jpg?token=private#secret",
        "darkImg":"https://music.126.net.example.invalid/dark.jpg",
        "lightGif":"https://p1.music.126.net/animation.gif?token=private",
        "vipAuthRespItem":{"authResult":{"canUse":true}},
        "soundMaterialList":[{"link":"https://example.invalid/private-material"}]})
}

fn detail() -> Value {
    json!({"code":200,"data":{"soundList":[
        {"soundId":"200","soundType":10,"pcSoundDetail":sound()}
    ]}})
}

fn auth(value: Value) -> Value {
    json!({"code":200,"data":[
        {"interestsId":200,"interestsType":116,"authResp":{"authResult":value}}
    ]})
}

fn library(
    api: Arc<ScriptApi>,
    directory: &std::path::Path,
) -> (Arc<AccountSession>, EffectsLibrary) {
    let store = Arc::new(StateStore::open(directory).unwrap());
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let library = EffectsLibrary::new(session.clone()).unwrap();
    (session, library)
}

#[tokio::test]
async fn one_catalogue_request_and_explicit_pc_detail_project_no_native_fields() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":[sound()]}).into(),
        detail().into(),
        auth(json!({"canUse":true,"canUseType":50})).into(),
    ]));
    let (session, library) = library(api.clone(), root.path());
    let token = session.token().unwrap();
    let rows = library
        .directory_for(&token, CatalogueKind::Recommended)
        .await
        .unwrap();
    assert_eq!(api.calls().len(), 1);
    assert_eq!(
        serde_json::to_value(rows).unwrap(),
        json!([{"id":200,"name":"Synthetic","subtitle":"Spatial","light_artwork":"https://p1.music.126.net/cover.jpg"}])
    );
    let view = library
        .directory_detail_for(&token, EffectId::new(200).unwrap())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(view).unwrap(),
        json!({"id":200,"name":"Synthetic","subtitle":"Spatial","light_artwork":"https://p1.music.126.net/cover.jpg","access":"trial"})
    );
    let calls = api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/usertool/sound/queryAllSound",
            "/api/usertool/home/sound/info",
            "/api/vipauth/app/interests/auth/query"
        ]
    );
    assert!(calls.iter().all(|call| !call.cache));
    assert_eq!(calls[0].payload, json!({"exposeType":6}));
    let requested_detail: Value =
        serde_json::from_str(calls[1].payload["reqListStr"].as_str().unwrap()).unwrap();
    assert_eq!(requested_detail, json!([{"soundId":"200","soundType":10}]));
    assert_eq!(
        calls[2].payload,
        json!({"queryInfo":{"interestsMap":{"116":[200]}}})
    );
}

#[tokio::test]
async fn absent_and_malformed_rights_are_distinct_and_neither_grants_access() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        detail().into(),
        json!({"code":200,"data":[]}).into(),
        detail().into(),
        auth(json!({"canUse":"true","canUseType":50})).into(),
    ]));
    let (session, library) = library(api, root.path());
    let token = session.token().unwrap();
    let id = EffectId::new(200).unwrap();
    for expected in [
        EffectDirectoryAccess::Unavailable,
        EffectDirectoryAccess::Unknown,
    ] {
        let row = library
            .directory_detail_for(&token, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.access, expected);
    }
}

#[tokio::test]
async fn late_rights_observation_cannot_cross_account_generation() {
    let root = tempfile::tempdir().unwrap();
    let (pending, send) = gate();
    let api = Arc::new(ScriptApi::new([detail().into(), pending]));
    let (session, library) = library(api.clone(), root.path());
    let token = session.token().unwrap();
    let reading = tokio::spawn(async move {
        library
            .directory_detail_for(&token, EffectId::new(200).unwrap())
            .await
    });
    api.wait_for_calls(2).await;
    session.detach().unwrap();
    send.send(Ok(auth(json!({"canUse":true})))).unwrap();
    assert!(matches!(reading.await.unwrap(), Err(Error::StaleResponse)));
}
