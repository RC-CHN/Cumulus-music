use cumulus_core::{
    Error,
    protocol::api::{ApiFuture, identifier, official_text_length, playlist_name},
    protocol::context::{RequestContext, TokenProvider, business_for_path},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const HOST: &str = "https://interfacepc.music.163.com";

#[test]
fn path_matching_agrees_with_frozen_official_module_402() {
    let cases: Value =
        serde_json::from_str(include_str!("fixtures/official-context.json")).unwrap();
    for case in cases.as_array().unwrap() {
        for (path, expected) in case["paths"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["expected"].as_array().unwrap())
        {
            let actual = business_for_path(path.as_str().unwrap(), &case["config"], HOST).unwrap();
            assert_eq!(
                actual.as_deref(),
                expected.as_str().filter(|s| !s.is_empty()),
                "path={path}, config={}",
                case["config"]
            );
        }
    }
}

struct Tokens(Mutex<Vec<String>>);
impl TokenProvider for Tokens {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            assert_eq!(business, "test-business");
            Ok(self.0.lock().unwrap().remove(0))
        })
    }
}

#[tokio::test]
async fn context_injects_fresh_tokens_in_both_places_without_mutating_input() {
    let tokens = Arc::new(Tokens(Mutex::new(vec![
        "one".into(),
        "two".into(),
        "".into(),
        "bad\r\nheader".into(),
    ])));
    let context = RequestContext::new(
        json!({"yidun#paths":{"whiteList":[{"businessId":"test-business","list":["/api/write"]}]}}),
        HOST.into(),
        tokens.clone(),
    );
    let source = json!({"nested":{"items":[1,2]}});
    for expected in ["one", "two"] {
        let mut result = context.prepare("/api/write", &source).await.unwrap();
        assert_eq!(result.payload["checkToken"], expected);
        assert_eq!(result.headers["X-antiCheatToken"], expected);
        result.payload["nested"]["items"][0] = json!(99);
        assert_eq!(source, json!({"nested":{"items":[1,2]}}));
    }
    assert!(context.prepare("/api/write", &source).await.is_err());
    assert!(context.prepare("/api/write", &source).await.is_err());
    let plain = context.prepare("/api/read", &source).await.unwrap();
    assert_eq!(plain.payload, source);
    assert!(plain.headers.is_empty());
    assert!(tokens.0.lock().unwrap().is_empty());
    assert!(context.prepare("/api/write?x=1", &source).await.is_err());
    assert!(context.prepare("/api/write", &json!([])).await.is_err());
}

#[test]
fn malformed_configuration_and_origin_lookalikes_do_not_acquire_a_token() {
    assert!(
        business_for_path(
            "/api/write",
            &json!({"yidun#paths":{"whiteList":[{"list":["/api/write"],"businessId":7}]}}),
            HOST
        )
        .is_err()
    );
    assert_eq!(
        business_for_path(
            "https://interfacepc.music.163.com.attacker.invalid/api/write",
            &json!({}),
            HOST
        )
        .unwrap(),
        None
    );
}

#[test]
fn ids_and_weighted_playlist_names_keep_the_recovered_input_boundary() {
    for value in [
        json!(true),
        json!(0),
        json!("000"),
        json!(-1),
        json!(1.5),
        json!("１２"),
        json!("1e2"),
    ] {
        assert!(identifier(&value).is_err());
    }
    for value in ["001", "18446744073709551616000"] {
        assert_eq!(identifier(&json!(value)).unwrap(), value);
    }
    assert_eq!(official_text_length("é中🎵"), 7);
    assert_eq!(playlist_name("\u{1c}  音乐 \u{1f}").unwrap(), "音乐");
    assert!(playlist_name(&"中".repeat(21)).is_err());
    assert!(playlist_name("a#b").is_err());
    assert!(playlist_name(&"🎵".repeat(10)).is_ok());
}

#[test]
fn printable_errors_omit_secret_response_and_transport_data() {
    for error in [
        Error::service("/api/login", json!(8821), json!({"token":"SECRET"})),
        Error::Transport("SECRET".into()),
    ] {
        assert!(!format!("{error:?}: {error}").contains("SECRET"));
    }
}
