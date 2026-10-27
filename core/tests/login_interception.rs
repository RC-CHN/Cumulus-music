mod support;
use cumulus_core::{
    Error, Result,
    account::login_policy::{
        InvalidLoginController, InvalidLoginOutcome, InvalidLoginSkip, LoginExperiments,
        TokenPresence,
    },
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use support::{Reply, ScriptApi, gate};
struct ClockApi {
    inner: ScriptApi,
    clock: Arc<AtomicU64>,
    finish: Option<u64>,
}
impl Api for ClockApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(finish) = self.finish {
                self.clock.store(finish, Ordering::SeqCst);
            }
            self.inner.request(path, payload, cache).await
        })
    }
}
#[tokio::test]
async fn all_recovered_original_function_scenarios_match_native_state_machine_and_requests() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/login_interception.json")).unwrap();
    for scenario in fixture["scenarios"].as_array().unwrap() {
        let clock = Arc::new(AtomicU64::new(100000));
        let machine = InvalidLoginController::with_clock({
            let c = clock.clone();
            Arc::new(move || Ok(c.load(Ordering::SeqCst)))
        });
        for step in scenario["steps"].as_array().unwrap() {
            let input = &step["input"];
            if let Some(now) = input["now"].as_u64() {
                clock.store(now, Ordering::SeqCst);
            }
            let tokens = if input["tokens"].is_object() {
                TokenPresence {
                    music_u: input["tokens"]["music_u"] == true,
                    music_a: input["tokens"]["music_a"] == true,
                }
            } else {
                TokenPresence {
                    music_u: true,
                    music_a: false,
                }
            };
            let response = if input["error"] == true {
                Reply::Ready(Err(Error::Transport("fixture failed".into())))
            } else {
                input
                    .get("response")
                    .cloned()
                    .unwrap_or(json!({"code":200,"data":{"action":1}}))
                    .into()
            };
            let api = ClockApi {
                inner: ScriptApi::new([response]),
                clock: clock.clone(),
                finish: input["finish"].as_u64(),
            };
            let config: Result<Value> = if input["configError"] == true {
                Err(Error::State("fixture config failure"))
            } else {
                Ok(input["config"].clone())
            };
            let outcome = machine
                .handle(
                    &api,
                    tokens,
                    input["url"].as_str().unwrap_or("/api/test"),
                    config,
                )
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(outcome).unwrap(),
                step["expected"],
                "scenario {} input {}",
                scenario["name"],
                input
            );
            let calls = api.inner.calls();
            assert_eq!(calls.len(), step["calls"].as_array().unwrap().len());
            for (call, original) in calls.iter().zip(step["calls"].as_array().unwrap()) {
                assert_eq!(call.path, original["path"]);
                assert_eq!(call.payload, original["data"]);
                assert!(!call.cache);
            }
        }
    }
    for row in fixture["gates"].as_array().unwrap() {
        let flags=LoginExperiments::from_response(&json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":row["jsk"]},{"expName":"PH-PC-newLoginBenefit","expGroupName":row["benefit"]}]})).unwrap();
        assert_eq!(
            row["inited"] == true && flags.enabled(),
            row["enabled"] == true
        );
    }
    for row in fixture["paths"].as_array().unwrap() {
        assert_eq!(
            cumulus_core::account::login_policy::normalized_path(row["input"].as_str().unwrap()),
            row["output"]
        );
    }
}
#[tokio::test]
async fn pending_intercept_is_single_flight_and_cancel_releases_it_without_replaying_business() {
    let machine = Arc::new(InvalidLoginController::default());
    let (pending, mut sender) = gate();
    let api = Arc::new(ScriptApi::new([
        pending,
        json!({"code":200,"data":{"action":1}}).into(),
    ]));
    let tokens = TokenPresence {
        music_u: true,
        music_a: false,
    };
    let task = {
        let m = machine.clone();
        let a = api.clone();
        tokio::spawn(async move {
            m.handle(a.as_ref(), tokens, "/api/source", Ok(Value::Null))
                .await
        })
    };
    api.wait_for_calls(1).await;
    assert!(matches!(
        machine
            .handle(api.as_ref(), tokens, "/api/other", Ok(Value::Null))
            .await
            .unwrap(),
        InvalidLoginOutcome::Skipped(InvalidLoginSkip::InProgress)
    ));
    task.abort();
    let _ = task.await;
    sender.closed().await;
    assert!(matches!(
        machine
            .handle(api.as_ref(), tokens, "/api/other", Ok(Value::Null))
            .await
            .unwrap(),
        InvalidLoginOutcome::Valid
    ));
    assert_eq!(api.calls().len(), 2);
    assert!(
        api.calls()
            .iter()
            .all(|c| c.path == "/api/middle/account/token/refresh")
    );
}
#[tokio::test]
async fn notice_contains_only_bounded_display_text_not_jump_urls_or_tokens() {
    let api=ScriptApi::new([json!({"code":200,"data":{"action":2,"message":"MUSIC_U=private","title":"x".repeat(513),"jumpUrl":"https://music.163.com/?token=private","buttonText":"private"}}).into()]);
    let outcome = InvalidLoginController::default()
        .handle(
            &api,
            TokenPresence {
                music_u: true,
                music_a: false,
            },
            "/api/test",
            Ok(Value::Null),
        )
        .await
        .unwrap();
    let value = serde_json::to_value(outcome).unwrap();
    assert_eq!(value["value"]["message"], "登录状态已失效，请重新登录");
    assert_eq!(value["value"]["title"], Value::Null);
    assert!(!value.to_string().contains("private"));
}
#[test]
fn experiment_first_match_missing_groups_and_invalid_shapes_do_not_fabricate_assignment() {
    let missing = LoginExperiments::from_response(&json!({"code":200,"data":[]})).unwrap();
    assert!(!missing.enabled());
    let duplicate=LoginExperiments::from_response(&json!({"code":200,"data":[{"expName":"PH-PC-loginJsk","expGroupName":"c"},{"expName":"PH-PC-loginJsk","expGroupName":"t1"},{"expName":"PH-PC-newLoginBenefit","expGroupName":"t1"}]})).unwrap();
    assert!(!duplicate.enabled());
    assert!(LoginExperiments::from_response(&json!({"code":200,"data":{}})).is_err());
    assert_eq!(
        LoginExperiments::payload(),
        json!({"expNames":"PH-PC-loginJsk,PH-PC-newLoginBenefit"})
    );
}
