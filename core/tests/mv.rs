mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession,
        resources::{AccountResources, MutationPhase, ResourceKind},
    },
    mv::{MvApi, MvDetail, MvPurpose, MvService},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};
const LIST: &str = "/api/mlog/mycollect/bytime/get";
fn detail(id: u64) -> Value {
    json!({"id":id,"resource":{"data":{"id":id,"name":"Video","artists":[{"id":7,"name":"Artist"}],"duration":123456,"brs":[{"br":480,"size":124.0,"point":0},{"br":240,"size":64,"point":0}]},"mp":{"fee":0,"payed":0,"pl":480,"dl":480}}})
}
fn page(ids: &[u64], more: bool, time: Value) -> Reply {
    json!({"code":200,"data":{"feeds":ids.iter().map(|id|detail(*id)).collect::<Vec<_>>(),"more":more,"time":time}}).into()
}
fn auth(id: u64) -> Value {
    json!({"code":200,"data":{"id":id,"code":200,"r":240,"size":63,"url":"https://v.test.vod.126.net/video?opaque=fixture","md5":""}})
}
fn world(api: Arc<ScriptApi>, store: Arc<StateStore>) -> Arc<AccountSession> {
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session
        .adopt(api, json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    session
}
#[test]
fn official_normalizer_and_download_choices_match_executed_javascript() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/mv.json")).unwrap();
    for row in fixture["cases"].as_array().unwrap() {
        let model = MvDetail::from_official(&row["input"]).unwrap();
        let output = serde_json::to_value(&model).unwrap();
        for (key, expected) in row["expected"].as_object().unwrap() {
            if key == "privilege" && expected.is_object() {
                for (field, value) in expected.as_object().unwrap() {
                    assert_eq!(
                        &output[key][field], value,
                        "{} privilege {field}",
                        row["name"]
                    );
                }
            } else {
                assert_eq!(&output[key], expected, "{} {key}", row["name"]);
            }
        }
        assert_eq!(
            serde_json::to_value(model.download_options()).unwrap(),
            row["download_options"],
            "{}",
            row["name"]
        );
    }
    assert_eq!(
        fixture["calls"][0]["payload"],
        json!({"id":"123","videoType":"mp4","type":"2"})
    );
    assert_eq!(fixture["calls"][1]["payload"], json!({"id":"123","r":240}));
    assert_eq!(
        fixture["calls"][2]["payload"],
        json!({"id":"123","brs":720,"type":"mp4"})
    );
}
#[test]
fn null_arrays_are_empty_but_identity_numeric_and_resource_type_corruption_fail() {
    let mut input = detail(123);
    input["resource"]["data"]["artists"] = Value::Null;
    input["resource"]["data"]["relateSong"] = Value::Null;
    input["resource"]["data"]["brs"] = Value::Null;
    let model = MvDetail::from_official(&input).unwrap();
    assert!(
        model.artists.is_empty() && model.bitrates.is_empty() && model.related_songs.is_empty()
    );
    for (path, bad) in [
        ("/resource/data/id", json!(124)),
        ("/resource/data/duration", json!(-1)),
        ("/resource/data/brs/0/size", json!(1.5)),
        ("/resource/data/artists", json!({})),
    ] {
        let mut input = detail(123);
        *input.pointer_mut(path).unwrap() = bad;
        assert!(MvDetail::from_official(&input).is_err(), "{path}");
    }
    let mut input = detail(123);
    input["resource"]["mlogBaseData"] = json!({"type":2});
    assert!(MvDetail::from_official(&input).is_err());
    let mut input = detail(123);
    input["resource"]["data"]["brs"] = json!([{"br":240},{"br":240}]);
    assert!(MvDetail::from_official(&input).is_err());
}
#[tokio::test]
async fn collection_reads_all_pages_preserves_cursor_type_order_and_membership() {
    let api = Arc::new(ScriptApi::new([
        page(&[3, 2], true, json!(900)),
        page(&[1], false, json!(800)),
    ]));
    let rows = MvApi::new(api.clone()).collection().await.unwrap();
    assert_eq!(
        rows.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
        ["3", "2", "1"]
    );
    assert!(rows.iter().all(|v| v.subscribed));
    assert_eq!(
        api.calls()[0].payload,
        json!({"time":"-1","mlogType":3,"limit":200})
    );
    assert_eq!(
        api.calls()[1].payload,
        json!({"time":900,"mlogType":3,"limit":200})
    );
    assert!(api.calls().iter().all(|v| v.path == LIST && !v.cache));
}
#[tokio::test]
async fn incomplete_collection_never_overwrites_last_complete_cache() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        page(&[3, 2], true, json!(900)),
        Reply::Ready(Err(Error::Transport("fixture".into()))),
    ]));
    store
        .put("1", "resources", "mvs", &json!([{"id":"99"}]))
        .unwrap();
    let session = world(api, store.clone());
    let resources = AccountResources::new(session).unwrap();
    assert!(resources.refresh(ResourceKind::Mvs).await.is_err());
    assert_eq!(
        store.get("1", "resources", "mvs").unwrap(),
        Some(json!([{"id":"99"}]))
    );
    for pages in [
        vec![page(&[3], true, json!(900)), page(&[2], true, json!(900))],
        vec![page(&[3], true, json!(900)), page(&[3], false, json!(800))],
        vec![page(&[], true, json!(900))],
    ] {
        assert!(
            MvApi::new(Arc::new(ScriptApi::new(pages)))
                .collection()
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn detail_and_authorizations_use_separate_official_endpoints_and_validate_inner_result() {
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":detail(123)}).into(),
        json!({"code":200,"data":detail(123)}).into(),
        auth(123).into(),
        auth(123).into(),
    ]));
    let mv = MvApi::new(api.clone());
    mv.detail(&json!(123)).await.unwrap();
    mv.download_options(&json!(123)).await.unwrap();
    mv.authorize(&json!(123), 240, MvPurpose::Play)
        .await
        .unwrap();
    mv.authorize(&json!(123), 240, MvPurpose::Download)
        .await
        .unwrap();
    assert_eq!(api.calls()[0].payload, json!({"id":"123","type":"2"}));
    assert_eq!(
        api.calls()[1].payload,
        json!({"id":"123","type":"2","videoType":"mp4"})
    );
    assert_eq!(api.calls()[2].path, MvPurpose::Play.path());
    assert_eq!(api.calls()[3].path, MvPurpose::Download.path());
    for (field, bad) in [
        ("id", json!(124)),
        ("code", json!(403)),
        ("url", json!("https://example.com/secret")),
        ("size", json!(0)),
        ("r", json!(0)),
        ("md5", json!("invalid")),
        ("freeTrialInfo", json!({"end":30})),
    ] {
        let mut response = auth(123);
        response["data"][field] = bad;
        let mv = MvApi::new(Arc::new(ScriptApi::new([response.into()])));
        assert!(
            mv.authorize(&json!(123), 240, MvPurpose::Download)
                .await
                .is_err(),
            "{field}"
        );
    }
    let api = Arc::new(ScriptApi::new([]));
    let mv = MvApi::new(api.clone());
    assert!(
        mv.authorize(&json!(123), 0, MvPurpose::Download)
            .await
            .is_err()
    );
    assert!(api.calls().is_empty());
}
#[tokio::test]
async fn uncertain_mv_collection_write_survives_reopen_then_reconciles_without_resending() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        page(&[10], false, Value::Null),
        Reply::Ready(Err(Error::Transport("lost acknowledgement".into()))),
    ]));
    let session = world(api.clone(), store);
    let resources = AccountResources::new(session).unwrap();
    assert!(
        resources
            .set_membership(ResourceKind::Mvs, &json!(20), true)
            .await
            .is_err()
    );
    assert_eq!(
        resources.operations().unwrap()["mvs:20"].phase,
        MutationPhase::Uncertain
    );
    assert_eq!(api.calls()[1].path, "/api/mv/sub");
    assert_eq!(api.calls()[1].payload, json!({"mvId":"20"}));
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        page(&[20, 10], false, Value::Null),
        page(&[20, 10], false, Value::Null),
        json!({"code":200}).into(),
        page(&[10], false, Value::Null),
    ]));
    let resources = AccountResources::new(world(api.clone(), store)).unwrap();
    assert!(matches!(
        resources
            .set_membership(ResourceKind::Mvs, &json!(20), false)
            .await,
        Err(Error::MutationUncertain)
    ));
    assert!(api.calls().is_empty());
    assert_eq!(
        resources
            .reconcile(ResourceKind::Mvs, "20")
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(
        resources
            .set_membership(ResourceKind::Mvs, &json!(20), false)
            .await
            .unwrap()
            .phase,
        MutationPhase::Confirmed
    );
    assert_eq!(api.calls()[2].path, "/api/mv/unsub");
    assert_eq!(api.calls()[2].payload, json!({"mvIds":["20"]}));
}
#[tokio::test]
async fn stale_mv_detail_cannot_repopulate_another_account_and_positions_are_persistent() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let (pending, release) = gate();
    let api = Arc::new(ScriptApi::new([pending]));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let service = Arc::new(MvService::new(session.clone()).unwrap());
    service
        .save_position_for(&token, &json!(123), 4567, 123456, false)
        .unwrap();
    let work = {
        let s = service.clone();
        let t = token.clone();
        tokio::spawn(async move { s.detail_for(&t, &json!(123)).await })
    };
    api.wait_for_calls(1).await;
    session
        .adopt(api.clone(), json!({"userId":2}), &token)
        .unwrap();
    release
        .send(Ok(json!({"code":200,"data":detail(123)})))
        .unwrap();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    let current = session.token().unwrap();
    assert!(service.cached_for(&current, &json!(123)).unwrap().is_none());
    assert!(
        service
            .position_for(&current, &json!(123))
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        service.save_position_for(&token, &json!(123), 5000, 123456, false),
        Err(Error::StaleResponse)
    ));
    let reopened = world(api, Arc::new(StateStore::open(root.path()).unwrap()));
    let token = reopened.token().unwrap();
    let service = MvService::new(reopened).unwrap();
    assert_eq!(
        service
            .position_for(&token, &json!(123))
            .unwrap()
            .unwrap()
            .position_ms,
        4567
    );
    assert_eq!(
        service
            .save_position_for(&token, &json!(123), 123456, 123456, true)
            .unwrap()
            .position_ms,
        0
    );
    assert!(
        service
            .save_position_for(&token, &json!(123), 123457, 123456, false)
            .is_err()
    );
    assert!(store.values("2", "mv-details-v1").unwrap().is_empty());
}
