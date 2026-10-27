mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    podcast::scene::{SceneFmApi, SceneFmService},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};
fn voice(id: u64) -> Value {
    json!({"id":id,"name":"Voice","mainSong":{"id":id+100,"duration":120000},"radio":{"id":7},"duration":120000})
}
fn home() -> Reply {
    json!({"code":200,"data":[{"creatives":[
    {"creativeId":"OP_76001","uiElement":{"mainTitle":{"title":"人间剧场"},"labelTexts":["3人在听"]},"creativeExtInfoVO":{"djPrograms":[voice(1),voice(2)]}},
    {"creativeId":"OP_7","uiElement":{"mainTitle":{"title":"新闻资讯"}}}
]}]}).into()
}
fn fixture(
    api: Arc<ScriptApi>,
) -> (
    tempfile::TempDir,
    Arc<StateStore>,
    Arc<AccountSession>,
    SceneFmService,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let s = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    s.adopt(api, json!({"userId":9}), &s.token().unwrap())
        .unwrap();
    let service = SceneFmService::new(s.clone()).unwrap();
    (dir, store, s, service)
}
#[tokio::test]
async fn category_and_full_queue_selection_match_executed_official_model() {
    let oracle: Value =
        serde_json::from_str(include_str!("fixtures/scene-fm-official.json")).unwrap();
    for case in oracle["cases"].as_array().unwrap() {
        let mut replies = vec![home()];
        if case["change"] == true {
            replies.push(json!({"code":200,"data":{"list":[voice(3),voice(4)]}}).into());
        }
        let api = Arc::new(ScriptApi::new(replies));
        let (_dir, store, s, service) = fixture(api);
        let t = s.token().unwrap();
        for id in case["heard"].as_array().unwrap() {
            store
                .put("9", "podcast-heard-v1", &id.to_string(), &json!(true))
                .unwrap();
        }
        let mut v = service.refresh_for(&t).await.unwrap();
        if case["change"] == true {
            v = service.select_for(&t, v.revision, "OP_7").await.unwrap();
        }
        let (v, resources, index) = service.prepare_for(&t, v.revision).await.unwrap();
        let e = &case["expected"];
        assert_eq!(json!(v.active_id), e["active"]);
        assert_eq!(v.selected().unwrap().id, e["selected"].as_str().unwrap());
        assert_eq!(
            json!(
                resources
                    .iter()
                    .map(|r| r["voice_id"].clone())
                    .collect::<Vec<_>>()
            ),
            e["voice_ids"]
        );
        assert_eq!(json!(index), e["index"]);
        assert_eq!(e["force_clear"], true);
        assert_eq!(e["scene"], "sceneFm");
        assert_eq!(e["play_count"], 1);
    }
}
#[tokio::test]
async fn official_first_unheard_uses_local_history_and_categories_load_lazily() {
    let api = Arc::new(ScriptApi::new([
        home(),
        json!({"code":200,"data":{"list":[voice(3),voice(4)]}}).into(),
    ]));
    let (_dir, store, s, service) = fixture(api.clone());
    let t = s.token().unwrap();
    store
        .put("9", "podcast-positions-v1", "1", &json!({"voiceId":"1"}))
        .unwrap();
    let v = service.refresh_for(&t).await.unwrap();
    assert_eq!(v.selected().unwrap().id, "2");
    assert_eq!(v.scenes[0].label, "3人在听");
    assert!(!service.autoplay_for(&t).unwrap());
    assert_eq!(api.calls().len(), 1);
    assert_eq!(
        api.calls()[0].payload,
        json!({"blockCode":"VOICE_HOMEPAGE_BLOCK_SCENE_FM"})
    );
    let selected = service.select_for(&t, v.revision, "OP_7").await.unwrap();
    assert!(selected.pending);
    assert_eq!(selected.selected().unwrap().id, "3");
    assert_eq!(api.calls()[1].path, "/api/content/scene/fm/play/list");
    assert_eq!(
        api.calls()[1].payload,
        json!({"sceneCode":"SCENE_FM","sceneId":"OP_7"})
    );
    let (p, resources, index) = service.prepare_for(&t, selected.revision).await.unwrap();
    assert_eq!(resources.len(), 2);
    assert_eq!(index, 0);
    assert_eq!(resources[0]["id"], "103");
    assert_eq!(resources[0]["voice_id"], "3");
    assert_eq!(api.calls().len(), 2);
    assert!(
        service
            .sync_for(&t, "OP_76001", "2", Some(p.revision))
            .unwrap()
            .unwrap()
            .pending
    );
    assert!(
        service
            .sync_for(&t, "OP_7", "4", Some(p.revision))
            .unwrap()
            .unwrap()
            .pending
    );
    assert!(
        service
            .sync_for(&t, "OP_7", "3", Some(p.revision - 1))
            .unwrap()
            .unwrap()
            .pending
    );
    let done = service
        .sync_for(&t, "OP_7", "3", Some(p.revision))
        .unwrap()
        .unwrap();
    assert!(!done.pending);
    assert!(done.revision > p.revision);
    assert_eq!(
        service
            .sync_for(&t, "OP_7", "4", Some(p.revision))
            .unwrap()
            .unwrap()
            .selected()
            .unwrap()
            .id,
        "4"
    );
}
#[tokio::test]
async fn pending_selection_survives_restart_requires_explicit_retry_or_cancel() {
    let api = Arc::new(ScriptApi::new([home(), home()]));
    let (_dir, store, s, service) = fixture(api.clone());
    let t = s.token().unwrap();
    let v = service.refresh_for(&t).await.unwrap();
    let (p, _, _) = service.prepare_for(&t, v.revision).await.unwrap();
    drop(service);
    let service = SceneFmService::new(s.clone()).unwrap();
    assert!(service.cached_for(&t).unwrap().unwrap().pending);
    assert!(service.refresh_for(&t).await.is_err());
    assert_eq!(api.calls().len(), 1);
    assert!(matches!(
        service.cancel_for(&t, v.revision),
        Err(Error::QueueChanged)
    ));
    service.cancel_for(&t, p.revision).unwrap();
    service.set_autoplay_for(&t, true).unwrap();
    let v = service.refresh_for(&t).await.unwrap();
    assert!(!v.pending);
    assert!(
        SceneFmService::new(s.clone())
            .unwrap()
            .autoplay_for(&t)
            .unwrap()
    );
    s.adopt(api.clone(), json!({"userId":10}), &t).unwrap();
    let next = s.token().unwrap();
    assert!(service.cached_for(&next).unwrap().is_none());
    assert!(!service.autoplay_for(&next).unwrap());
    assert!(service.cached_for(&t).is_err());
    assert!(store.get("9", "scene-fm-v1", "current").unwrap().is_some());
}
#[tokio::test]
async fn failed_refresh_and_late_other_account_response_cannot_replace_cache() {
    let (pending, send) = gate();
    let api = Arc::new(ScriptApi::new([
        home(),
        json!({"code":200,"data":[{"creatives":[{"creativeId":"OP_7"},{"creativeId":"OP_7"}]}]})
            .into(),
        pending,
    ]));
    let (_dir, store, s, service) = fixture(api.clone());
    let t = s.token().unwrap();
    service.refresh_for(&t).await.unwrap();
    let old = store.get("9", "scene-fm-v1", "current").unwrap();
    assert!(service.refresh_for(&t).await.is_err());
    assert_eq!(old, store.get("9", "scene-fm-v1", "current").unwrap());
    let task = {
        let s = s.clone();
        let t = t.clone();
        tokio::spawn(async move { SceneFmService::new(s).unwrap().refresh_for(&t).await })
    };
    api.wait_for_calls(3).await;
    s.adopt(api.clone(), json!({"userId":10}), &t).unwrap();
    let Reply::Ready(Ok(reply)) = home() else {
        panic!()
    };
    send.send(Ok(reply)).unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(old, store.get("9", "scene-fm-v1", "current").unwrap());
    assert!(store.get("10", "scene-fm-v1", "current").unwrap().is_none());
}
#[tokio::test]
async fn listened_preview_is_skipped_and_all_heard_falls_back_to_first() {
    let api = Arc::new(ScriptApi::new([home()]));
    let (_dir, store, s, service) = fixture(api.clone());
    let t = s.token().unwrap();
    store
        .put("9", "podcast-heard-v1", "1", &json!(true))
        .unwrap();
    let v = service.refresh_for(&t).await.unwrap();
    assert_eq!(v.selected().unwrap().id, "2");
    store
        .put("9", "podcast-heard-v1", "2", &json!(true))
        .unwrap();
    let v = service
        .select_for(&t, v.revision, "OP_76001")
        .await
        .unwrap();
    assert_eq!(v.selected().unwrap().id, "1");
    assert_eq!(api.calls().len(), 1);
}
#[tokio::test]
async fn empty_or_malformed_programs_never_publish_partial_queue() {
    for list in [
        json!([voice(3), voice(3)]),
        json!([voice(3),{"id":null}]),
        json!({}),
    ] {
        let api = Arc::new(ScriptApi::new([
            home(),
            json!({"code":200,"data":{"list":list}}).into(),
        ]));
        let (_dir, store, s, service) = fixture(api);
        let t = s.token().unwrap();
        let v = service.refresh_for(&t).await.unwrap();
        let old = store.get("9", "scene-fm-v1", "current").unwrap();
        assert!(service.select_for(&t, v.revision, "OP_7").await.is_err());
        assert_eq!(old, store.get("9", "scene-fm-v1", "current").unwrap());
    }
    let api = Arc::new(ScriptApi::new([
        home(),
        json!({"code":200,"data":{"list":[]}}).into(),
        json!({"code":200,"data":{"list":[]}}).into(),
    ]));
    let (_dir, _store, s, service) = fixture(api);
    let t = s.token().unwrap();
    let v = service.refresh_for(&t).await.unwrap();
    let v = service.select_for(&t, v.revision, "OP_7").await.unwrap();
    assert!(v.selected().is_none());
    assert!(service.prepare_for(&t, v.revision).await.is_err());
    assert!(service.cached_for(&t).unwrap().unwrap().pending);
}
#[tokio::test]
async fn arbitrary_scene_ids_and_stale_revisions_are_rejected_before_network() {
    let api = Arc::new(ScriptApi::new([home()]));
    let (_dir, _store, s, service) = fixture(api.clone());
    let t = s.token().unwrap();
    let v = service.refresh_for(&t).await.unwrap();
    for id in ["", "../OP_7", "OP_999"] {
        assert!(service.select_for(&t, v.revision, id).await.is_err());
    }
    assert!(matches!(
        service.select_for(&t, 0, "OP_7").await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls().len(), 1);
    assert!(SceneFmApi::new(api).programs("场景").await.is_err());
}
