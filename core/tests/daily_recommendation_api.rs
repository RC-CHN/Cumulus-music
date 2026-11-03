mod support;

use cumulus_core::{
    Error,
    catalog::{CatalogApi, DailyRecommendationEndpoint as Endpoint},
};
use serde_json::json;
use std::sync::Arc;
use support::ScriptApi;

#[tokio::test]
async fn official_daily_groups_use_one_exact_route_and_its_own_response_shape() {
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"recommend":[{"id":4}],"data":{"showFreeTag":true,"dailySongs":[{"id":90}]}}).into(),
        json!({"code":200,"recommend":[{"id":90}],"data":{"dailySongs":[{"id":5}],"showFreeTag":false}}).into(),
        json!({"code":200,"data":11}).into(),
        json!({"code":"301"}).into(),
    ]));
    let catalog = CatalogApi::new(api.clone());
    let control = catalog.daily_for(Endpoint::V1).await.unwrap();
    assert_eq!(control.songs, vec![json!({"id":4})]);
    assert!(control.show_free_tag);
    let treated = catalog.daily_for(Endpoint::V3).await.unwrap();
    assert_eq!(treated.songs, vec![json!({"id":5})]);
    assert!(!treated.show_free_tag);
    assert!(matches!(
        catalog.daily_for(Endpoint::V3).await,
        Err(Error::Response(_))
    ));
    assert!(
        catalog
            .daily_for(Endpoint::V1)
            .await
            .err()
            .unwrap()
            .is_service_code(301)
    );
    let calls = api.calls();
    assert_eq!(calls.len(), 4);
    for (call, version) in calls.iter().zip(["v1", "v3", "v3", "v1"]) {
        assert_eq!(
            call.path,
            format!("/api/{version}/discovery/recommend/songs")
        );
        assert_eq!(call.payload, json!({"limit":30}));
        assert!(!call.cache);
    }
}
