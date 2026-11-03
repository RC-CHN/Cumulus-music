mod support;
use cumulus_core::{
    Error,
    account::{
        AccountSession,
        credentials::Credentials,
        login_policy::LoginExperiments,
        refresh::{AccountLifecycle, DeviceInfo, DeviceTokenProvider, RefreshOutcome},
    },
    protocol::{
        api::ApiFuture,
        http::{ApiOrigin, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::http::{HttpResponse, ScriptHttp, reply};

struct Provider {
    calls: AtomicUsize,
    value: String,
}
impl DeviceTokenProvider for Provider {
    fn token(&self) -> ApiFuture<'_, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(self.value.clone()) })
    }
}

#[tokio::test]
async fn rust_refresh_matches_full_recovered_method_control_and_request_vectors() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/refresh-control-official.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = &case["input"];
        let expected = &case["expected"];
        if !input["enabled"].as_bool().unwrap() {
            // Native renewal wraps this primitive in the actual two gates;
            // client_login_policy separately exercises its HTTP assignment.
            assert!(!LoginExperiments::default().enabled());
            assert_eq!(expected["outcome"], "policy_disabled");
            assert!(expected["calls"].as_array().unwrap().is_empty());
            continue;
        }
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let credentials = Credentials::open(store.clone(), ApiOrigin::Pc).unwrap();
        let old_u = input["musicU"].as_str().unwrap();
        let old_rt = input["refreshToken"].as_str().unwrap();
        let cookies = if old_u.is_empty() {
            vec![]
        } else {
            vec![format!("MUSIC_U={old_u}; Path=/; Secure")]
        };
        let before = credentials.login_credentials().unwrap();
        credentials
            .record_response(
                &ApiOrigin::Pc.url(),
                &cookies,
                (!old_rt.is_empty()).then_some(old_rt),
                &before.revision,
            )
            .unwrap();
        // Seed a frozen synthetic installation/login clock, then reopen it as
        // a real native process would. Never import real account material.
        let mut raw = store
            .get("transport", "credentials", "session")
            .unwrap()
            .unwrap();
        raw["device_id"] = input["device"]["deviceId"].clone();
        let last = input["lastMs"].as_u64().unwrap() / 1000;
        raw["last_login_at"] = if last == 0 { Value::Null } else { json!(last) };
        store
            .put("transport", "credentials", "session", &raw)
            .unwrap();
        let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc).unwrap());
        let identity = || json!({"code":200,"profile":{"userId":123}}).into();
        let mut responses = vec![identity()];
        if !expected["calls"].as_array().unwrap().is_empty() {
            if input["transportError"] == true {
                responses.push(HttpResponse::Ready(Err(Error::Transport(
                    "synthetic transport".into(),
                ))));
            } else {
                let mut response = reply(json!({"code":input["code"]}));
                let new_u = input["newMusicU"].as_str().unwrap();
                let new_rt = input["newRefreshToken"].as_str().unwrap();
                if !new_u.is_empty() {
                    response.set_cookies = vec![format!("MUSIC_U={new_u}; Path=/; Secure")];
                }
                response.refresh_token = (!new_rt.is_empty()).then(|| new_rt.to_owned());
                responses.push(HttpResponse::Ready(Ok(response)));
            }
        }
        if expected["outcome"] == "refreshed" {
            responses.push(identity());
        }
        let http = Arc::new(ScriptHttp::new(responses));
        let client = Arc::new(
            MusicHttpClient::with_backend(
                credentials.clone(),
                Default::default(),
                Default::default(),
                None,
                http.clone(),
            )
            .unwrap(),
        );
        let session = Arc::new(AccountSession::new(client.clone(), Some(store)));
        session.refresh().await.unwrap();
        let now = input["nowMs"].as_u64().unwrap() / 1000;
        let lifecycle = AccountLifecycle::with_clock(session, client, Arc::new(move || Ok(now)));
        let device = DeviceInfo {
            device_name: input["device"]["deviceName"].as_str().unwrap().into(),
            device_display_name: input["device"]["deviceDisplayName"]
                .as_str()
                .unwrap()
                .into(),
            device_id: input["device"]["deviceId"].as_str().unwrap().into(),
            os: input["device"]["os"].as_str().unwrap().into(),
            client_type: (input["envError"] != true)
                .then(|| input["env"]["clientType"].as_str().unwrap().into()),
        };
        let provider = Provider {
            calls: AtomicUsize::new(0),
            value: input["ydToken"].as_str().unwrap().into(),
        };
        let result = lifecycle.refresh_jsk(&device, &provider, false).await;
        let actual = match result {
            Ok(RefreshOutcome::ReauthenticationRequired) => json!("missing_pair"),
            Ok(value) => serde_json::to_value(value).unwrap(),
            Err(_) => json!("request_failed"),
        };
        assert_eq!(actual, expected["outcome"], "{name}");
        assert_eq!(
            provider.calls.load(Ordering::SeqCst) as u64,
            expected["tokenReads"].as_u64().unwrap(),
            "{name}"
        );
        let refreshes: Vec<_> = http
            .calls()
            .into_iter()
            .filter(|call| call.path == "/api/login/token/refresh/v1")
            .collect();
        assert_eq!(
            refreshes.len(),
            expected["calls"].as_array().unwrap().len(),
            "{name}"
        );
        if let Some(call) = refreshes.first() {
            let mut actual = call.payload.clone();
            for key in ["header", "e_r", "fromPage"] {
                actual.as_object_mut().unwrap().remove(key);
            }
            let mut payload = expected["calls"][0]["payload"].clone();
            for value in payload.as_object_mut().unwrap().values_mut() {
                if value.is_number() {
                    *value = value.to_string().into();
                }
            }
            assert_eq!(actual, payload, "{name} full official payload/signature");
            assert_eq!(call.payload["fromPage"], "JSK");
            assert_eq!(call.login_chain.as_deref(), Some(""));
        }
        let after = credentials.login_credentials().unwrap();
        let status = lifecycle.refresh_status().unwrap();
        if actual == "refreshed" {
            assert_eq!(status.last_success, Some(now), "{name}");
            assert_eq!(
                after.refresh_token.as_deref(),
                input["newRefreshToken"].as_str(),
                "{name}"
            );
        } else {
            assert!(status.last_success.is_none(), "{name}");
            assert_eq!(
                after.refresh_token.as_deref(),
                (!old_rt.is_empty()).then_some(old_rt),
                "{name}: original refreshToken must survive incomplete/rejected receipts"
            );
        }
        if !refreshes.is_empty() && actual != "refreshed" {
            assert!(status.pending, "{name}");
            assert_eq!(
                lifecycle
                    .refresh_jsk(&device, &provider, false)
                    .await
                    .unwrap(),
                RefreshOutcome::RecoveryRequired,
                "{name}"
            );
            assert_eq!(
                http.calls().len(),
                2,
                "{name}: unknown result never replayed"
            );
        }
    }
}
