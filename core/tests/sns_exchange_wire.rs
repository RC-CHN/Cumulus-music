//! Offline JSK SNS request conformance. This binary has no callback producer,
//! account store, backend, or runtime entry point.
use cumulus_core::{
    account::credentials::RequestCredentials,
    protocol::{
        http::{
            ApiOrigin, ClientProfile, EapiStyle, RequestInput, RequestOptions, WireRoute,
            build_request,
        },
        wire::{
            eapi,
            serialization::{official_data, official_form},
        },
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::form_urlencoded;

const PATH: &str = "/api/login/sns";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Provider {
    Weixin,
    Qq,
}
impl Provider {
    fn kind(self) -> u8 {
        match self {
            Self::Weixin => 10,
            Self::Qq => 5,
        }
    }
    fn other(self) -> Self {
        match self {
            Self::Weixin => Self::Qq,
            Self::Qq => Self::Weixin,
        }
    }
}

#[derive(Deserialize)]
struct Corpus {
    kind: String,
    method: String,
    path: String,
    cases: [Vector; 2],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vector {
    provider: Provider,
    #[serde(rename = "type")]
    kind: u8,
    oauth_query: String,
    desktop_result: Value,
    expected_form: String,
}

/// Provenance is the test vector, not the JSON shape. There is no conversion
/// from a provider redirect, arbitrary callback JSON, or a production caller.
struct FrozenDesktopTicket<'a> {
    provider: Provider,
    object: &'a Value,
}

impl FrozenDesktopTicket<'_> {
    fn post_payload(self, selected: Provider) -> Option<Value> {
        if self.provider.kind() != selected.kind() {
            return None;
        }
        // These fixtures intentionally use strings. The real producer's code
        // type and the rest of its object are not established by these tests.
        let desktop_code = self
            .object
            .get("code")?
            .as_str()
            .filter(|v| !v.is_empty())?;
        Some(json!({
            "type": selected.kind(),
            "tokenJsonStr": desktop_code,
            "desktopLoginSnsParam": self.object,
            "fromPage": "JSK",
        }))
    }
}

impl Vector {
    fn frozen_ticket(&self) -> FrozenDesktopTicket<'_> {
        FrozenDesktopTicket {
            provider: self.provider,
            object: &self.desktop_result,
        }
    }
    fn oauth_code(&self) -> String {
        let [(key, value)]: [_; 1] = form_urlencoded::parse(self.oauth_query.as_bytes())
            .into_owned()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_eq!(key, "code");
        value
    }
    fn oracle_fields(&self) -> Value {
        // Independent standard percent decoding of the frozen M/48 form.
        Value::Object(
            form_urlencoded::parse(self.expected_form.as_bytes())
                .into_owned()
                .map(|(key, value)| (key, Value::String(value)))
                .collect(),
        )
    }
}

fn corpus() -> [Vector; 2] {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/sns-exchange-wire.json")).unwrap();
    assert_eq!(corpus.kind, "synthetic-only");
    assert_eq!(
        (corpus.method.as_str(), corpus.path.as_str()),
        ("POST", PATH)
    );
    assert_eq!(corpus.cases.each_ref().map(|vector| vector.kind), [10, 5]);
    corpus.cases
}

#[test]
fn sns_form_keeps_opaque_desktop_code_and_serializes_the_desktop_object_once() {
    for vector in corpus() {
        let payload = vector
            .frozen_ticket()
            .post_payload(vector.provider)
            .unwrap();
        assert_eq!(payload["type"], vector.kind);
        assert_eq!(payload["tokenJsonStr"], vector.desktop_result["code"]);
        assert_eq!(payload["desktopLoginSnsParam"], vector.desktop_result);
        assert_ne!(payload["tokenJsonStr"], vector.oauth_code());
        assert_eq!(official_form(&payload).unwrap(), vector.expected_form);

        let normalized = official_data(&payload).unwrap();
        assert_eq!(normalized, vector.oracle_fields());
        assert_eq!(normalized["tokenJsonStr"], vector.desktop_result["code"]);
        assert_eq!(
            serde_json::from_str::<Value>(normalized["desktopLoginSnsParam"].as_str().unwrap())
                .unwrap(),
            vector.desktop_result,
            "decode once to an object, with the synthetic opaque data unchanged"
        );
    }
}

#[test]
fn sns_frontend_eapi_plan_contains_one_body_and_no_provider_code_or_account_credentials() {
    for vector in corpus() {
        let payload = vector
            .frozen_ticket()
            .post_payload(vector.provider)
            .unwrap();
        let origin = ApiOrigin::Pc;
        let credentials = RequestCredentials {
            target: origin.request_url(WireRoute::Eapi, PATH).unwrap(),
            device_id: "synthetic-sns-device".into(),
            cookies: Vec::new(),
            revision: "synthetic-fresh-candidate".into(),
        };
        let profile = ClientProfile {
            user_agent: "synthetic-sns-wire".into(),
            os_version: "synthetic-windows".into(),
            client_sign: Some("synthetic-sns-sign".into()),
        };
        let headers = BTreeMap::from([("x-login-chain-id".into(), "synthetic-sns-chain".into())]);
        let plan = build_request(RequestInput {
            origin,
            path: PATH,
            payload,
            credentials: &credentials,
            profile: &profile,
            extra_headers: &headers,
            options: RequestOptions {
                style: EapiStyle::Frontend,
                ..Default::default()
            },
        })
        .unwrap();
        assert_eq!(plan.route, WireRoute::Eapi);
        assert_eq!(
            plan.url.as_str(),
            "https://interfacepc.music.163.com/eapi/login/sns"
        );
        assert_eq!(
            plan.headers["Content-Type"],
            "application/x-www-form-urlencoded"
        );
        assert_eq!(plan.headers["x-login-chain-id"], "synthetic-sns-chain");
        let [(field, params)]: [_; 1] = form_urlencoded::parse(&plan.body)
            .into_owned()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert_eq!(field, "params");
        let (path, mut body) = eapi::unwrap(&params).unwrap();
        assert_eq!(path, PATH);
        let encrypted_context = body.as_object_mut().unwrap().remove("header").unwrap();
        let encrypted_context: Value =
            serde_json::from_str(encrypted_context.as_str().unwrap()).unwrap();
        assert_eq!(encrypted_context["x-login-chain-id"], "synthetic-sns-chain");
        assert_eq!(body, vector.oracle_fields());
        assert_ne!(body["tokenJsonStr"], vector.oauth_code());
        for name in ["MUSIC_U", "MUSIC_A", "NMTID", "MUSIC_TU"] {
            assert!(encrypted_context.get(name).is_none());
            assert!(!plan.headers["Cookie"].split(';').any(|pair| {
                pair.trim()
                    .split_once('=')
                    .is_some_and(|(key, _)| key == name)
            }));
        }
    }
}

#[test]
fn fixture_tickets_are_bound_to_the_selected_provider_and_require_a_desktop_code() {
    for vector in corpus() {
        assert!(
            vector
                .frozen_ticket()
                .post_payload(vector.provider.other())
                .is_none()
        );
        for object in [json!({}), json!({ "code": "" }), json!(vector.oauth_code())] {
            assert!(
                FrozenDesktopTicket {
                    provider: vector.provider,
                    object: &object,
                }
                .post_payload(vector.provider)
                .is_none()
            );
        }
    }
}
