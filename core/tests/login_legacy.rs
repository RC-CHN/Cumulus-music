mod support;
use cumulus_core::{
    Error, Result,
    account::login_policy::LegacyLoginController,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use support::{Reply, ScriptApi, gate};
struct ClockApi {
    api: ScriptApi,
    clock: Arc<AtomicU64>,
    finish: Option<u64>,
}
impl Api for ClockApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(finish) = self.finish {
                self.clock.store(finish, Ordering::SeqCst);
            }
            self.api.request(path, payload, cache).await
        })
    }
}
#[tokio::test]
async fn original_authenticated_subscriber_cases_match_including_completion_timer_and_strict_null()
{
    let fixture: Value = serde_json::from_str(include_str!("fixtures/login_legacy.json")).unwrap();
    for scenario in fixture["scenarios"].as_array().unwrap() {
        if scenario["difference"] == true {
            continue;
        }
        let clock = Arc::new(AtomicU64::new(0));
        let controller = LegacyLoginController::with_clock({
            let c = clock.clone();
            Arc::new(move || Ok(c.load(Ordering::SeqCst)))
        });
        for step in scenario["steps"].as_array().unwrap() {
            let input = &step["input"];
            if let Some(now) = input["now"].as_u64() {
                clock.store(now, Ordering::SeqCst);
            }
            let api = ClockApi {
                api: ScriptApi::new([input
                    .get("response")
                    .cloned()
                    .unwrap_or(json!({"code":200,"data":{"action":1}}))
                    .into()]),
                clock: clock.clone(),
                finish: input["finish"].as_u64(),
            };
            let actual = controller.handle(&api).await.unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                step["expected"],
                "{} {}",
                scenario["name"],
                input
            );
            let calls = api.api.calls();
            assert_eq!(calls.len(), step["calls"].as_array().unwrap().len());
            for call in calls {
                assert_eq!(call.path, "/api/middle/account/token/refresh");
                assert_eq!(call.payload, json!({}));
                assert!(!call.cache);
            }
        }
    }
}
#[tokio::test]
async fn exceptions_and_cancellation_release_the_original_stuck_latch_without_counting_or_replay() {
    let machine = Arc::new(LegacyLoginController::default());
    let (pending, mut sender) = gate();
    let api = Arc::new(ScriptApi::new([
        Reply::Ready(Err(Error::Transport("synthetic failure".into()))),
        pending,
        json!({"code":200,"data":{"action":null}}).into(),
    ]));
    assert!(machine.handle(api.as_ref()).await.is_err());
    let task = {
        let m = machine.clone();
        let a = api.clone();
        tokio::spawn(async move { m.handle(a.as_ref()).await })
    };
    api.wait_for_calls(2).await;
    assert_eq!(
        serde_json::to_value(machine.handle(api.as_ref()).await.unwrap()).unwrap(),
        json!({"kind":"skipped","value":"in_progress"})
    );
    task.abort();
    let _ = task.await;
    sender.closed().await;
    assert_eq!(
        serde_json::to_value(machine.handle(api.as_ref()).await.unwrap()).unwrap(),
        json!({"kind":"failed","value":{"count":1}})
    );
    assert_eq!(api.calls().len(), 3);
}
#[tokio::test]
async fn clock_failure_and_untrusted_notice_are_bounded_without_overriding_local_error() {
    let controller =
        LegacyLoginController::with_clock(Arc::new(|| Err(Error::State("clock unavailable"))));
    let api = ScriptApi::new([]);
    let result: Result<_> = controller.handle(&api).await;
    assert!(matches!(result, Err(Error::State(_))));
    assert!(api.calls().is_empty());
    let api=ScriptApi::new([json!({"code":200,"data":{"action":2,"title":"MUSIC_U=private","message":"x-refresh-token:private","jumpUrl":"https://example.test/private"}}).into()]);
    let value =
        serde_json::to_value(LegacyLoginController::default().handle(&api).await.unwrap()).unwrap();
    assert_eq!(
        value["value"]["notice"],
        json!({"kind":"reauthenticate","title":null,"message":"用户验证失效，请重新登录"})
    );
    assert!(!value.to_string().contains("private"));
}
