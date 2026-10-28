use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    catalog::{
        AlbumDetailSource, CatalogApi,
        details::{CatalogDetails, CatalogQuery},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const V3: &str = "/api/album/v3/detail";
const V4: &str = "/api/album/v4/detail";
const DYNAMIC: &str = "/api/album/detail/dynamic";
const RIGHTS: &str = "/api/album/privilege";
const EXPOSURE: &str = "/api/resource-exposure/config";

struct Backend {
    responses: Mutex<BTreeMap<&'static str, Value>>,
    calls: Mutex<Vec<(String, Value, bool)>>,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload, cache));
            self.responses
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or(Error::Transport("fixture".into()))
        })
    }
}
fn fixture() -> (Arc<Backend>, CatalogApi) {
    let detail = json!({"code":200,"album":{"id":7,"name":"合成专辑","isSub":true,"commentCount":1,"info":{"commentCount":2,"likedCount":3,"subTime":4}},"songs":[{"id":1,"name":"甲","privilege":{"id":1,"pl":111}},{"id":2,"name":"乙","privilege":{"id":2,"pl":222}}]});
    let api = Arc::new(Backend {
        responses: Mutex::new([
            (V3,detail.clone()), (V4,detail),
            (DYNAMIC,json!({"code":200,"commentCount":9,"likedCount":0,"subCount":12,"isSub":false,"subTime":null})),
            (RIGHTS,json!({"code":200,"data":[{"id":2,"pl":320000},{"id":1,"pl":128000}]})),
            (EXPOSURE,json!({"code":200,"data":{"resourceType":"common"}})),
        ].into_iter().collect()),
        calls: Mutex::default(),
    });
    (api.clone(), CatalogApi::new(api))
}

#[tokio::test]
async fn official_page_sequence_and_dynamic_precedence_are_preserved() {
    let (api, catalog) = fixture();
    let page = catalog.album_details(&json!(7)).await.unwrap();
    assert_eq!(page.read.source, AlbumDetailSource::V3);
    assert!(page.read.privileges_refreshed);
    assert_eq!(page.read.digital_album_tag, Some(true));
    assert_eq!(page.response["album"]["commentCount"], 9);
    assert_eq!(page.response["album"]["likedCount"], 0);
    assert_eq!(page.response["album"]["isSub"], false);
    assert_eq!(page.response["album"]["subTime"], Value::Null);
    assert_eq!(page.response["songs"][0]["privilege"]["pl"], 128000);
    assert_eq!(page.response["songs"][1]["privilege"]["pl"], 320000);
    assert_eq!(
        *api.calls.lock().unwrap(),
        vec![
            (V3.into(), json!({"id":"7"}), true),
            (DYNAMIC.into(), json!({"id":"7"}), false),
            (RIGHTS.into(), json!({"id":"7"}), false),
            (
                EXPOSURE.into(),
                json!({"resourcePosition":"album","resourceId":"7","rexposureRecords":[]}),
                false
            ),
        ]
    );
}

#[tokio::test]
async fn v3_failure_uses_v4_once_without_reclassifying_a_successful_wrong_album() {
    for service_failure in [false, true] {
        let (api, catalog) = fixture();
        if service_failure {
            api.responses
                .lock()
                .unwrap()
                .insert(V3, json!({"code":403}));
        } else {
            api.responses.lock().unwrap().remove(V3);
        }
        let page = catalog.album_details(&json!(7)).await.unwrap();
        assert_eq!(page.read.source, AlbumDetailSource::V4);
        assert_eq!(
            api.calls.lock().unwrap()[1],
            (V4.into(), json!({"id":"7"}), false)
        );
        assert_eq!(api.calls.lock().unwrap().len(), 5);
    }
    let (api, catalog) = fixture();
    api.responses.lock().unwrap().get_mut(V3).unwrap()["album"]["id"] = json!(8);
    assert!(catalog.album_details(&json!(7)).await.is_err());
    assert_eq!(api.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn both_detail_failures_stop_and_invalid_input_never_reaches_transport() {
    let (api, catalog) = fixture();
    assert!(catalog.album_details(&json!("../7")).await.is_err());
    assert!(api.calls.lock().unwrap().is_empty());
    api.responses.lock().unwrap().remove(V3);
    api.responses
        .lock()
        .unwrap()
        .insert(V4, json!({"code":404}));
    assert!(
        catalog
            .album_details(&json!(7))
            .await
            .err()
            .unwrap()
            .is_service_code(404)
    );
    assert_eq!(api.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn optional_reads_keep_original_songs_and_distinguish_unavailable_from_false() {
    let (api, catalog) = fixture();
    api.responses.lock().unwrap().remove(RIGHTS);
    api.responses.lock().unwrap().remove(EXPOSURE);
    let page = catalog.album_details(&json!(7)).await.unwrap();
    assert!(!page.read.privileges_refreshed);
    assert_eq!(page.read.digital_album_tag, None);
    assert_eq!(page.response["songs"][0]["privilege"]["pl"], 111);
    // A partially valid array must not partially update the original songs.
    api.responses.lock().unwrap().insert(
        RIGHTS,
        json!({"code":200,"data":[{"id":1,"pl":900},{"id":"bad"}]}),
    );
    api.responses.lock().unwrap().insert(
        EXPOSURE,
        json!({"code":200,"data":{"resourceType":"other"}}),
    );
    let page = catalog.album_details(&json!(7)).await.unwrap();
    assert!(!page.read.privileges_refreshed);
    assert_eq!(page.read.digital_album_tag, Some(false));
    assert_eq!(page.response["songs"][0]["privilege"]["pl"], 111);
}

#[tokio::test]
async fn missing_dynamic_fields_and_empty_rights_do_not_preserve_stale_state() {
    let (api, catalog) = fixture();
    api.responses
        .lock()
        .unwrap()
        .insert(DYNAMIC, json!({"code":200}));
    api.responses
        .lock()
        .unwrap()
        .insert(RIGHTS, json!({"code":200}));
    let page = catalog.album_details(&json!(7)).await.unwrap();
    assert!(page.read.privileges_refreshed);
    assert!(page.response["album"].get("isSub").is_none());
    assert!(page.response["songs"][0].get("privilege").is_none());
    assert!(page.response["songs"][1].get("privilege").is_none());
}

#[tokio::test]
async fn required_dynamic_failure_preserves_the_previous_account_snapshot() {
    let (api, _) = fixture();
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let service = CatalogDetails::new(session.clone()).unwrap();
    let token = session.token().unwrap();
    let query = CatalogQuery::Album { id: "7".into() };
    let saved = service.page_for(&token, &query).await.unwrap();
    api.calls.lock().unwrap().clear();
    api.responses
        .lock()
        .unwrap()
        .insert(DYNAMIC, json!({"code":503}));
    assert!(service.page_for(&token, &query).await.is_err());
    assert_eq!(api.calls.lock().unwrap().len(), 2);
    let cached = service.cached_for(&token, &query).unwrap().unwrap();
    assert_eq!(cached.response, saved.response);
    assert_eq!(cached.album_read, saved.album_read);
    assert_eq!(cached.fetched_at, saved.fetched_at);
}
