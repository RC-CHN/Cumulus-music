use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    protocol::{
        api::{Api, ApiFuture},
        context::{RequestContext, TokenProvider},
        http::ApiOrigin,
        mconfig::{APPLICATION_KEY, ConfigBackend, ConfigUrl, PcConfig, Update},
    },
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
struct HomeApi(AtomicUsize);

impl Api for HomeApi {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            Ok(match path {
                "/api/toplist/detail/v2" => json!({"code":200,"data":[]}),
                "/api/feealbum/songsaleboard/daily/type" => json!({"code":200,"products":[]}),
                "/api/single/sales-ranking/get" => json!({"code":200,"data":{"itemList":[]}}),
                _ => panic!("unexpected synthetic request"),
            })
        })
    }
}

#[derive(Default)]
struct ConfigFixture(AtomicUsize);

impl ConfigBackend for ConfigFixture {
    fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            let common = if url.as_str().ends_with("/short.data") {
                json!(0.000_000_1)
            } else {
                // A nonnumeric value must use the default; neighboring keys
                // are deliberately set to the short duration.
                json!("0.0000001")
            };
            Ok(json!({"preload#cacheValidateTime":{
                "home":0.000_000_1,"fmPlay":0.000_000_1,"choice":{"common":common}
            }}))
        })
    }
}

struct NoToken;
impl TokenProvider for NoToken {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        panic!("cached rankings must not require a token")
    }
}

fn headers(version: u64, os: &str, object: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(
        "mconfig-info".into(),
        json!({APPLICATION_KEY:{
            "version":version,"os":os,
            "fullUrl":format!("https://d5.music.126.net/obj/fixture/{object}.data")
        }})
        .to_string(),
    )])
}

#[tokio::test]
async fn ranking_cache_reads_latest_verified_pc_choice_without_refreshing() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(HomeApi::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let backend = Arc::new(ConfigFixture::default());
    let config = PcConfig::new(registry.store(), backend.clone()).unwrap();
    let context = Arc::new(
        RequestContext::from_pc(config.clone(), "3.1.42.205594", Arc::new(NoToken)).unwrap(),
    );
    let mut client = MusicClient::with_session(session, registry);
    client.auth.context = Some(context.clone());
    let refreshed = client
        .ranking_page("1", RankingQuery::Home, true)
        .await
        .unwrap();
    assert!(refreshed.value.fresh);
    assert_eq!(
        config.resolve(&headers(1, "web", "short")).await.unwrap(),
        Update::NonPc
    );
    assert!(
        client
            .ranking_page("1", RankingQuery::Home, false)
            .await
            .unwrap()
            .value
            .fresh
    );
    assert_eq!(
        config.resolve(&headers(1, "pc", "short")).await.unwrap(),
        Update::Updated
    );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let stale = client
        .ranking_page("1", RankingQuery::Home, false)
        .await
        .unwrap();
    assert!(stale.value.cached && !stale.value.fresh);
    assert_eq!(stale.value.fetched_at, refreshed.value.fetched_at);
    client.auth.context = Some(Arc::new(RequestContext::new(
        json!({"preload#cacheValidateTime":{"choice":{"common":0.000_000_1}}}),
        ApiOrigin::Pc.as_str().into(),
        Arc::new(NoToken),
    )));
    assert!(
        client
            .ranking_page("1", RankingQuery::Home, false)
            .await
            .unwrap()
            .value
            .fresh
    );
    client.auth.context = Some(context);
    assert_eq!(
        config.resolve(&headers(2, "pc", "invalid")).await.unwrap(),
        Update::Updated
    );
    let fresh = client
        .ranking_page("1", RankingQuery::Home, false)
        .await
        .unwrap();
    assert!(fresh.value.cached && fresh.value.fresh);
    assert_eq!(fresh.value.fetched_at, refreshed.value.fetched_at);
    assert_eq!(api.0.load(Ordering::Relaxed), 3);
    assert_eq!(backend.0.load(Ordering::Relaxed), 2);
}
