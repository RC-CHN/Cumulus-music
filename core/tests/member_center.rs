mod support;
use cumulus_core::{
    Error, account::AccountSession, membership::center::MemberCenter, storage::StateStore,
};
use serde_json::json;
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

async fn setup(
    store: Arc<StateStore>,
    responses: impl IntoIterator<Item = Reply>,
) -> (Arc<AccountSession>, Arc<ScriptApi>, Arc<MemberCenter>) {
    let mut replies = vec![json!({"code":200,"profile":{"userId":1}}).into()];
    replies.extend(responses);
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.refresh().await.unwrap();
    let center = Arc::new(MemberCenter::new(session.clone()).unwrap());
    (session, api, center)
}

#[tokio::test]
async fn module_order_coalescing_ranking_and_failed_refresh_preserve_last_content() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let modules = json!({"code":200,"data":[{"moduleType":4},{"moduleType":9},{"moduleType":3},{"moduleType":1},{"moduleType":99}]});
    let (session, api, center) = setup(
        store,
        [
            modules.clone().into(),
            json!({"code":200,"data":[]}).into(),
            json!({"code":200,"data":{"songInfoList":[{"id":1}]}}).into(),
            json!({"code":200,"data":[{"contentList":[{"songId":2},{"songId":1}]}]}).into(),
            json!({"code":200,"songs":[{"id":1},{"id":2}]}).into(),
            json!({"code":200,"data":{"contentList":[]}}).into(),
            modules.into(),
            json!({"code":200,"data":[]}).into(),
            json!({"code":403}).into(),
        ],
    )
    .await;
    let result = center.refresh().await.unwrap();
    assert_eq!(
        result["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["module"]["moduleType"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![4, 9, 3, 1]
    );
    assert_eq!(
        api.calls()
            .iter()
            .filter(|c| c.path.ends_with("recommend/song/list"))
            .count(),
        1
    );
    assert_eq!(
        result["results"][1]["value"]["value"][0]["contentList"][0]["track"]["id"],
        2
    );
    assert_eq!(
        result["results"][3]["value"]["value"][0]["resourceType"],
        "PC_BACKUP"
    );
    assert!(center.refresh().await.unwrap_err().is_service_code(403));
    assert_eq!(center.snapshot().unwrap()["status"], "error");
    assert_eq!(center.snapshot().unwrap()["results"], result["results"]);
    session.detach().unwrap();
    assert!(matches!(center.snapshot(), Err(Error::StaleResponse)));
}

#[tokio::test]
async fn all_remaining_module_loaders_and_banner_actions_use_official_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let banners=vec![
        json!({"imgUrl":"https://example.invalid/song","resourceType":"SONG","resourceId":1}).to_string(),
        json!({"imgUrl":"https://example.invalid/playlist","resourceType":"PLAYLIST","resourceId":2}).to_string(),
        json!({"imgUrl":"https://example.invalid/activity","jumpUrl":"https://music.163.com/"}).to_string(),
        json!({"imgUrl":"https://example.invalid/excluded","resourceType":"SONG","resourceId":3,"algFlag":1}).to_string(),
        json!({"imgUrl":"","resourceType":"SONG","resourceId":4}).to_string(),
    ];
    let (_,api,center)=setup(store,[
        json!({"code":200,"data":[{"moduleType":1},{"moduleType":5},{"moduleType":6},{"moduleType":7},{"moduleType":12}]}).into(),
        json!({"code":200,"data":[]}).into(),json!({"code":200,"data":{"contentList":banners}}).into(),
        json!({"code":200,"data":{"blocks":[]}}).into(),json!({"code":200,"data":[{"id":5}]}).into(),
        json!({"code":200,"data":{"contentList":[json!({"imgUrl":"https://example.invalid/yes","jumpUrl":"https://music.163.com/"}).to_string(),json!({"imgUrl":"https://example.invalid/no"}).to_string()]}}).into(),
        json!({"code":200,"data":[{"id":6}]}).into(),
    ]).await;
    let result = center.refresh().await.unwrap();
    let rows = result["results"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    let top = rows[0]["value"]["value"].as_array().unwrap();
    assert_eq!(top.len(), 3);
    assert_eq!(
        top[0]["action"],
        json!({"kind":"play","resourceId":1,"resourceType":"track"})
    );
    assert_eq!(top[1]["action"]["linkPath"], "/playlist/2");
    assert_eq!(top[2]["resourceType"], "ACTIVITY");
    assert_eq!(rows[1]["value"]["kind"], "audioBooks");
    assert_eq!(rows[2]["value"]["kind"], "digitAlbums");
    assert_eq!(rows[3]["value"]["value"].as_array().unwrap().len(), 1);
    assert_eq!(rows[4]["value"]["kind"], "playlist");
    let calls = api.calls();
    assert_eq!(calls[3].payload, json!({"positionType":16}));
    assert_eq!(calls[4].path, "/api/podcast/common/single/block/get");
    assert_eq!(
        calls[4].payload,
        json!({"blockCode":"VIP_THEATER_VOICE_BOOK","pageCode":"HOMEPAGE_VOICELIST_RCMD"})
    );
    assert_eq!(calls[6].payload, json!({"positionType":18}));
}

#[tokio::test]
async fn history_dates_and_details_are_separate_and_bad_input_or_late_data_cannot_save() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (pending, release) = gate();
    let (session, api, center) = setup(
        store.clone(),
        [
            json!({"code":200,"data":{"dates":["2026-10-24"]}}).into(),
            json!({"code":200,"data":{"songs":[{"id":1}]}}).into(),
            pending,
        ],
    )
    .await;
    assert_eq!(
        center.history(None).await.unwrap(),
        vec![json!("2026-10-24")]
    );
    assert_eq!(
        center.history(Some(json!("2026-10-24"))).await.unwrap(),
        vec![json!({"id":1})]
    );
    assert_eq!(api.calls()[2].payload, json!({"date":"2026-10-24"}));
    for date in [
        json!(true),
        json!({}),
        json!("x".repeat(33)),
        json!(1.5),
        json!("dates"),
        json!(""),
    ] {
        assert!(matches!(
            center.history(Some(date)).await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(
        store.values("1", "recommendation_history").unwrap().len(),
        2
    );
    let work = {
        let center = center.clone();
        tokio::spawn(async move { center.history(Some(json!("2026-10-23"))).await })
    };
    api.wait_for_calls(4).await;
    session.detach().unwrap();
    assert!(
        release
            .send(Ok(json!({"code":200,"data":{"songs":[]}})))
            .is_ok()
    );
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        store.values("1", "recommendation_history").unwrap().len(),
        2
    );
}

#[tokio::test]
async fn concurrent_refresh_is_suppressed_and_aborted_loading_state_recovers_without_losing_content()
 {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (pending, _release) = gate();
    let (_, api, center) = setup(store.clone(), [pending]).await;
    store
        .put(
            "1",
            "member_center",
            "state",
            &json!({"status":"ready","results":[{"cached":true}]}),
        )
        .unwrap();
    let work = {
        let center = center.clone();
        tokio::spawn(async move { center.refresh().await })
    };
    api.wait_for_calls(2).await;
    assert_eq!(center.snapshot().unwrap()["status"], "loading");
    assert!(matches!(center.refresh().await, Err(Error::MutationBusy)));
    work.abort();
    assert!(work.await.is_err());
    let recovered = center.snapshot().unwrap();
    assert_eq!(recovered["status"], "error");
    assert_eq!(recovered["error"]["class"], "interrupted");
    assert_eq!(recovered["results"], json!([{"cached":true}]));
}
